//! A thin client for the few Stripe calls billing needs, plus webhook signature
//! checking. Requests pin [`API_VERSION`], so response shapes don't change when
//! the account's default version does; only the fields we read are parsed.

use std::collections::HashMap;

use hmac::{Hmac, KeyInit, Mac};
use serde::{Deserialize, de::DeserializeOwned};
use sha2::Sha256;
use uuid::Uuid;

/// The Stripe API version our requests are written against.
pub const API_VERSION: &str = "2026-08-26.dahlia";

/// How far a webhook's timestamp may be from our clock.
const WEBHOOK_TOLERANCE_SECONDS: i64 = 300;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("couldn't reach Stripe: {0}")]
    Http(#[from] reqwest::Error),
    #[error("Stripe answered {status}: {message}")]
    Api {
        status: u16,
        code: Option<String>,
        message: String,
    },
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    base: String,
    secret_key: String,
}

#[derive(Debug, Deserialize)]
pub struct Customer {
    pub id: String,
}

/// A Checkout or billing portal session; we only need where to send the browser.
#[derive(Debug, Deserialize)]
pub struct Session {
    pub url: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Price {
    pub id: String,
    /// In the currency's smallest unit (cents).
    pub unit_amount: Option<i64>,
    pub currency: String,
    pub recurring: Option<Recurring>,
    /// The product it belongs to (an ID unless expanded).
    #[serde(default)]
    pub product: Option<String>,
    #[serde(default = "yes")]
    pub active: bool,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Deserialize)]
pub struct Product {
    pub id: String,
}

/// What the Stripe Dashboard shows, for links from the admin section.
#[derive(Debug, Clone, Copy)]
pub enum DashboardPage {
    Customer,
    Subscription,
    Product,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Recurring {
    /// `day`, `week`, `month` or `year`.
    pub interval: String,
}

#[derive(Debug, Deserialize)]
pub struct List<T> {
    pub data: Vec<T>,
}

/// Any Stripe object, when only its ID matters.
#[derive(Debug, Deserialize)]
pub struct Id {
    pub id: String,
}

#[derive(Debug, Deserialize)]
pub struct Subscription {
    pub id: String,
    pub customer: String,
    pub status: String,
    #[serde(default)]
    pub cancel_at_period_end: bool,
    /// When it will cancel, if scheduled (the portal may set this instead of
    /// `cancel_at_period_end`).
    pub cancel_at: Option<i64>,
    pub canceled_at: Option<i64>,
    #[serde(default)]
    pub metadata: HashMap<String, String>,
    pub items: List<SubscriptionItem>,
}

#[derive(Debug, Deserialize)]
pub struct SubscriptionItem {
    pub id: String,
    pub price: Price,
    pub quantity: Option<i64>,
    pub current_period_start: Option<i64>,
    pub current_period_end: Option<i64>,
}

/// What to put in a Checkout Session for a new subscription.
pub struct CheckoutParams<'a> {
    pub customer: &'a str,
    /// Prices and quantities: a plan and/or add-ons, all with one interval.
    pub items: &'a [(String, i32)],
    pub workspace_id: Uuid,
    pub success_url: &'a str,
    pub cancel_url: &'a str,
}

#[derive(Deserialize)]
struct ErrorBody {
    error: ErrorDetail,
}

#[derive(Deserialize)]
struct ErrorDetail {
    code: Option<String>,
    message: Option<String>,
}

impl Client {
    pub fn new(http: reqwest::Client, base: &str, secret_key: &str) -> Self {
        Self {
            http,
            base: base.trim_end_matches('/').to_owned(),
            secret_key: secret_key.to_owned(),
        }
    }

    async fn send<T: DeserializeOwned>(&self, request: reqwest::RequestBuilder) -> Result<T> {
        let response = request
            .bearer_auth(&self.secret_key)
            .header("Stripe-Version", API_VERSION)
            .timeout(std::time::Duration::from_secs(20))
            .send()
            .await?;
        let status = response.status();
        if status.is_success() {
            return Ok(response.json().await?);
        }
        let body = response.json::<ErrorBody>().await.ok();
        Err(Error::Api {
            status: status.as_u16(),
            code: body.as_ref().and_then(|body| body.error.code.clone()),
            message: body
                .and_then(|body| body.error.message)
                .unwrap_or_else(|| status.to_string()),
        })
    }

    fn post<F: serde::Serialize + ?Sized>(&self, path: &str, form: &F) -> reqwest::RequestBuilder {
        self.http.post(format!("{}{path}", self.base)).form(form)
    }

    /// A customer for a workspace. Idempotent per workspace, so two checkouts
    /// started at once get the same customer.
    pub async fn create_customer(&self, name: &str, workspace_id: Uuid) -> Result<Customer> {
        let request = self
            .post(
                "/v1/customers",
                &[
                    ("name", name.to_owned()),
                    ("metadata[workspace_id]", workspace_id.to_string()),
                ],
            )
            .header("Idempotency-Key", format!("customer-{workspace_id}"));
        self.send(request).await
    }

    pub async fn create_checkout_session(&self, params: &CheckoutParams<'_>) -> Result<Session> {
        let workspace_id = params.workspace_id.to_string();
        let mut form: Vec<(String, String)> = vec![
            ("mode".into(), "subscription".into()),
            ("customer".into(), params.customer.to_owned()),
            ("allow_promotion_codes".into(), "true".into()),
            ("client_reference_id".into(), workspace_id.clone()),
            ("metadata[workspace_id]".into(), workspace_id.clone()),
            (
                "subscription_data[metadata][workspace_id]".into(),
                workspace_id,
            ),
            ("success_url".into(), params.success_url.to_owned()),
            ("cancel_url".into(), params.cancel_url.to_owned()),
        ];
        for (index, (price, quantity)) in params.items.iter().enumerate() {
            form.push((format!("line_items[{index}][price]"), price.clone()));
            form.push((
                format!("line_items[{index}][quantity]"),
                quantity.to_string(),
            ));
        }
        let request = self.post("/v1/checkout/sessions", &form);
        self.send(request).await
    }

    pub async fn create_portal_session(&self, customer: &str, return_url: &str) -> Result<Session> {
        let request = self.post(
            "/v1/billing_portal/sessions",
            &[
                ("customer", customer.to_owned()),
                ("return_url", return_url.to_owned()),
            ],
        );
        self.send(request).await
    }

    /// Changes a subscription (items, proration, cancellation) and returns it as it
    /// is now. With `payment_behavior=error_if_incomplete`, a declined charge
    /// fails the request (402) and changes nothing.
    pub async fn update_subscription(
        &self,
        id: &str,
        form: &[(String, String)],
    ) -> Result<Subscription> {
        let request = self.post(&format!("/v1/subscriptions/{id}"), form);
        self.send(request).await
    }

    pub async fn get_subscription(&self, id: &str) -> Result<Subscription> {
        let request = self
            .http
            .get(format!("{}/v1/subscriptions/{id}", self.base));
        self.send(request).await
    }

    /// Deletes a customer: its subscriptions end at once and its payment methods
    /// go (Stripe keeps its invoices, as it must). A customer that's already gone
    /// counts as deleted.
    pub async fn delete_customer(&self, id: &str) -> Result<()> {
        let request = self.http.delete(format!("{}/v1/customers/{id}", self.base));
        match self.send::<serde_json::Value>(request).await {
            Ok(_) => Ok(()),
            Err(Error::Api { status: 404, .. }) => Ok(()),
            Err(error) => Err(error),
        }
    }

    /// Keeps the customer's name (shown on invoices) in step with the workspace's.
    pub async fn set_customer_name(&self, id: &str, name: &str) -> Result<()> {
        let form = [("name", name.to_owned())];
        self.send::<serde_json::Value>(self.post(&format!("/v1/customers/{id}"), &form))
            .await
            .map(drop)
    }

    /// Whether the customer has a card (or another payment method) to charge.
    pub async fn has_payment_method(&self, customer: &str) -> Result<bool> {
        let page = self.payment_methods(customer, 1).await?;
        Ok(!page.data.is_empty())
    }

    async fn payment_methods(&self, customer: &str, limit: u8) -> Result<List<Id>> {
        let request = self
            .http
            .get(format!(
                "{}/v1/customers/{customer}/payment_methods",
                self.base
            ))
            .query(&[("limit", limit.to_string())]);
        self.send(request).await
    }

    /// Removes whatever the previous owner left on a workspace's customer when
    /// ownership moves: every payment method, the billing email, address, phone,
    /// shipping and tax IDs. The name becomes the workspace's again. `subscription`
    /// (the live one) stops pointing at a card, so the renewal uses whatever card
    /// the new owner adds. Finalized invoices keep what was on them. Safe to repeat.
    pub async fn remove_billing_details(
        &self,
        customer: &str,
        name: &str,
        subscription: Option<&str>,
    ) -> Result<()> {
        // Detached ones leave the list, so read the first page until it's empty.
        for _ in 0..20 {
            let page = self.payment_methods(customer, 100).await?;
            if page.data.is_empty() {
                break;
            }
            for method in page.data {
                let detach = self.post(
                    &format!("/v1/payment_methods/{}/detach", method.id),
                    &[] as &[(&str, &str)],
                );
                self.send::<serde_json::Value>(detach).await?;
            }
        }

        let request = self
            .http
            .get(format!("{}/v1/customers/{customer}/tax_ids", self.base))
            .query(&[("limit", "100")]);
        let tax_ids: List<Id> = self.send(request).await?;
        for tax_id in tax_ids.data {
            let delete = self.http.delete(format!(
                "{}/v1/customers/{customer}/tax_ids/{}",
                self.base, tax_id.id
            ));
            self.send::<serde_json::Value>(delete).await?;
        }

        // An empty value clears a field in Stripe.
        let form = [
            ("name", name.to_owned()),
            ("email", String::new()),
            ("phone", String::new()),
            ("address", String::new()),
            ("shipping", String::new()),
            ("invoice_settings[default_payment_method]", String::new()),
        ];
        self.send::<serde_json::Value>(self.post(&format!("/v1/customers/{customer}"), &form))
            .await?;

        if let Some(subscription) = subscription {
            let form = [("default_payment_method", String::new())];
            self.send::<serde_json::Value>(
                self.post(&format!("/v1/subscriptions/{subscription}"), &form),
            )
            .await?;
        }
        Ok(())
    }

    pub async fn get_price(&self, id: &str) -> Result<Price> {
        let request = self.http.get(format!("{}/v1/prices/{id}", self.base));
        self.send(request).await
    }

    /// A product for a plan or add-on, created from the admin section.
    pub async fn create_product(
        &self,
        name: &str,
        description: &str,
        unit_label: Option<&str>,
        kind: &str,
        code: &str,
    ) -> Result<Product> {
        let mut form = vec![
            ("name", name.to_owned()),
            ("metadata[kind]", kind.to_owned()),
            ("metadata[code]", code.to_owned()),
        ];
        if !description.is_empty() {
            form.push(("description", description.to_owned()));
        }
        if let Some(unit) = unit_label {
            form.push(("unit_label", unit.to_owned()));
        }
        self.send(self.post("/v1/products", &form)).await
    }

    pub async fn update_product(
        &self,
        id: &str,
        name: &str,
        description: &str,
        unit_label: Option<&str>,
        active: bool,
    ) -> Result<Product> {
        // An empty value clears a field in Stripe.
        let mut form = vec![
            ("name", name.to_owned()),
            ("description", description.to_owned()),
            ("active", active.to_string()),
        ];
        if let Some(unit) = unit_label {
            form.push(("unit_label", unit.to_owned()));
        }
        self.send(self.post(&format!("/v1/products/{id}"), &form))
            .await
    }

    /// A flat-rate recurring price (per unit) on `product`.
    pub async fn create_price(
        &self,
        product: &str,
        amount: i32,
        currency: &str,
        interval: &str,
    ) -> Result<Price> {
        let form = [
            ("product", product.to_owned()),
            ("unit_amount", amount.to_string()),
            ("currency", currency.to_owned()),
            ("recurring[interval]", interval.to_owned()),
        ];
        self.send(self.post("/v1/prices", &form)).await
    }

    /// Hides a product from new use in Stripe (existing subscriptions are unaffected).
    pub async fn archive_product(&self, id: &str) -> Result<Product> {
        let form = [("active", "false".to_owned())];
        self.send(self.post(&format!("/v1/products/{id}"), &form))
            .await
    }

    /// Stops a price from being used for new subscriptions (existing ones keep it).
    pub async fn archive_price(&self, id: &str) -> Result<Price> {
        let form = [("active", "false".to_owned())];
        self.send(self.post(&format!("/v1/prices/{id}"), &form))
            .await
    }

    /// Where the Stripe Dashboard shows an object, in test or live mode to match the key.
    pub fn dashboard_url(&self, page: DashboardPage, id: &str) -> String {
        dashboard_url(&self.secret_key, page, id)
    }
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum WebhookError {
    #[error("missing or malformed Stripe-Signature header")]
    Malformed,
    #[error("the timestamp is too far from now")]
    Stale,
    #[error("no signature matches")]
    Mismatch,
}

/// Test-mode keys (`sk_test_…`, `rk_test_…`) link to the test Dashboard.
pub fn dashboard_url(secret_key: &str, page: DashboardPage, id: &str) -> String {
    let test = secret_key.starts_with("sk_test_") || secret_key.starts_with("rk_test_");
    let section = match page {
        DashboardPage::Customer => "customers",
        DashboardPage::Subscription => "subscriptions",
        DashboardPage::Product => "products",
    };
    format!(
        "https://dashboard.stripe.com/{}{section}/{id}",
        if test { "test/" } else { "" }
    )
}

/// Checks a webhook's `Stripe-Signature` header (`t=…,v1=…[,v1=…]`): one `v1`
/// must be the HMAC-SHA256 of `"{t}.{body}"` with the endpoint's secret, and
/// `t` within five minutes of `now` (Unix seconds).
pub fn verify_webhook(
    header: &str,
    body: &[u8],
    secret: &str,
    now: i64,
) -> std::result::Result<(), WebhookError> {
    let mut timestamp = None;
    let mut signatures = Vec::new();
    for part in header.split(',') {
        match part.trim().split_once('=') {
            Some(("t", value)) => timestamp = value.parse::<i64>().ok(),
            Some(("v1", value)) => signatures.push(value),
            _ => {}
        }
    }
    let timestamp = timestamp.ok_or(WebhookError::Malformed)?;
    if signatures.is_empty() {
        return Err(WebhookError::Malformed);
    }
    if (now - timestamp).abs() > WEBHOOK_TOLERANCE_SECONDS {
        return Err(WebhookError::Stale);
    }

    for signature in signatures {
        let Some(expected) = decode_hex(signature) else {
            continue;
        };
        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
            .expect("HMAC takes keys of any length");
        mac.update(timestamp.to_string().as_bytes());
        mac.update(b".");
        mac.update(body);
        // Constant-time comparison.
        if mac.verify_slice(&expected).is_ok() {
            return Ok(());
        }
    }
    Err(WebhookError::Mismatch)
}

fn decode_hex(value: &str) -> Option<Vec<u8>> {
    if value.len() % 2 != 0 {
        return None;
    }
    (0..value.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(value.get(i..i + 2)?, 16).ok())
        .collect()
}

/// Signs a webhook body like Stripe does, for tests.
#[cfg(test)]
pub fn sign_webhook(body: &[u8], secret: &str, timestamp: i64) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(format!("{timestamp}.").as_bytes());
    mac.update(body);
    let signature: String = mac
        .finalize()
        .into_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("t={timestamp},v1={signature}")
}

/// The parts of a webhook event we use.
#[derive(Debug, Deserialize)]
pub struct Event {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub data: EventData,
}

#[derive(Debug, Deserialize)]
pub struct EventData {
    pub object: serde_json::Value,
}

impl Event {
    /// The subscription an event is about, for the events billing follows.
    pub fn subscription_id(&self) -> Option<&str> {
        let object = &self.data.object;
        match self.kind.as_str() {
            "checkout.session.completed" => object["subscription"].as_str(),
            kind if kind.starts_with("customer.subscription.") => object["id"].as_str(),
            // Newer API versions moved it under `parent`.
            "invoice.paid" | "invoice.payment_failed" => {
                object["parent"]["subscription_details"]["subscription"]
                    .as_str()
                    .or_else(|| object["subscription"].as_str())
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{DashboardPage, Event, WebhookError, dashboard_url, sign_webhook, verify_webhook};

    const SECRET: &str = "whsec_test";
    const BODY: &[u8] = br#"{"id":"evt_1","type":"customer.subscription.updated"}"#;

    #[test]
    fn dashboard_links_follow_the_key_mode() {
        assert_eq!(
            dashboard_url("sk_test_abc", DashboardPage::Customer, "cus_1"),
            "https://dashboard.stripe.com/test/customers/cus_1"
        );
        assert_eq!(
            dashboard_url("rk_live_abc", DashboardPage::Subscription, "sub_1"),
            "https://dashboard.stripe.com/subscriptions/sub_1"
        );
        assert_eq!(
            dashboard_url("sk_live_abc", DashboardPage::Product, "prod_1"),
            "https://dashboard.stripe.com/products/prod_1"
        );
    }

    #[test]
    fn a_correct_signature_passes() {
        let header = sign_webhook(BODY, SECRET, 1_790_000_000);
        assert_eq!(verify_webhook(&header, BODY, SECRET, 1_790_000_010), Ok(()));
    }

    #[test]
    fn any_of_several_signatures_may_match() {
        let good = sign_webhook(BODY, SECRET, 1_790_000_000);
        let header = format!("t=1790000000,v1={},{}", "ab".repeat(32), &good[13..]);
        assert_eq!(verify_webhook(&header, BODY, SECRET, 1_790_000_000), Ok(()));
    }

    #[test]
    fn wrong_secret_body_or_time_fails() {
        let header = sign_webhook(BODY, SECRET, 1_790_000_000);
        assert_eq!(
            verify_webhook(&header, BODY, "whsec_other", 1_790_000_000),
            Err(WebhookError::Mismatch)
        );
        assert_eq!(
            verify_webhook(&header, b"{}", SECRET, 1_790_000_000),
            Err(WebhookError::Mismatch)
        );
        assert_eq!(
            verify_webhook(&header, BODY, SECRET, 1_790_000_000 + 301),
            Err(WebhookError::Stale)
        );
        assert_eq!(
            verify_webhook("v1=abcd", BODY, SECRET, 0),
            Err(WebhookError::Malformed)
        );
        assert_eq!(
            verify_webhook("t=1790000000,v1=zz", BODY, SECRET, 1_790_000_000),
            Err(WebhookError::Mismatch)
        );
    }

    fn event(kind: &str, object: serde_json::Value) -> Event {
        serde_json::from_value(serde_json::json!({
            "id": "evt_1", "type": kind, "data": { "object": object }
        }))
        .unwrap()
    }

    #[test]
    fn events_point_at_their_subscription() {
        let checkout = event(
            "checkout.session.completed",
            serde_json::json!({ "id": "cs_1", "subscription": "sub_1" }),
        );
        assert_eq!(checkout.subscription_id(), Some("sub_1"));
        let updated = event(
            "customer.subscription.updated",
            serde_json::json!({ "id": "sub_2" }),
        );
        assert_eq!(updated.subscription_id(), Some("sub_2"));
        let invoice = event(
            "invoice.payment_failed",
            serde_json::json!({ "id": "in_1", "parent": { "subscription_details": { "subscription": "sub_3" } } }),
        );
        assert_eq!(invoice.subscription_id(), Some("sub_3"));
        let other = event("charge.refunded", serde_json::json!({ "id": "ch_1" }));
        assert_eq!(other.subscription_id(), None);
    }
}

#[cfg(test)]
mod transfer_tests {
    use std::sync::{Arc, Mutex};

    use axum::{
        Json, Router,
        body::Bytes,
        extract::State,
        http::{Method, Uri},
    };
    use serde_json::{Value, json};

    use super::Client;

    /// A Stripe with two cards and a tax ID on `cus_1`, recording every request.
    #[derive(Clone, Default)]
    struct Fake {
        log: Arc<Mutex<Vec<String>>>,
        cards: Arc<Mutex<Vec<&'static str>>>,
    }

    async fn handle(
        State(fake): State<Fake>,
        method: Method,
        uri: Uri,
        body: Bytes,
    ) -> Json<Value> {
        let body = String::from_utf8_lossy(&body).into_owned();
        let path = uri.path().to_owned();
        fake.log
            .lock()
            .unwrap()
            .push(format!("{method} {path} {body}").trim_end().to_owned());
        let mut cards = fake.cards.lock().unwrap();
        Json(match (method, path.as_str()) {
            (Method::GET, "/v1/customers/cus_1/payment_methods") => {
                json!({ "data": cards.iter().map(|id| json!({ "id": id })).collect::<Vec<_>>() })
            }
            (Method::POST, detach) if detach.ends_with("/detach") => {
                cards.retain(|id| !detach.contains(id));
                json!({})
            }
            (Method::GET, "/v1/customers/cus_1/tax_ids") => json!({ "data": [{ "id": "txi_1" }] }),
            _ => json!({}),
        })
    }

    #[tokio::test]
    async fn a_transfer_removes_every_card_and_the_billing_details() {
        let fake = Fake::default();
        fake.cards.lock().unwrap().extend(["pm_a", "pm_b"]);
        let app = Router::new().fallback(handle).with_state(fake.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await });

        let client = Client::new(reqwest::Client::new(), &base, "sk_test_x");
        assert!(client.has_payment_method("cus_1").await.unwrap());
        client
            .remove_billing_details("cus_1", "Kate Live", Some("sub_1"))
            .await
            .unwrap();
        assert!(!client.has_payment_method("cus_1").await.unwrap());

        let log = fake.log.lock().unwrap().clone();
        assert_eq!(
            log[1..log.len() - 1],
            [
                "GET /v1/customers/cus_1/payment_methods",
                "POST /v1/payment_methods/pm_a/detach",
                "POST /v1/payment_methods/pm_b/detach",
                "GET /v1/customers/cus_1/payment_methods",
                "GET /v1/customers/cus_1/tax_ids",
                "DELETE /v1/customers/cus_1/tax_ids/txi_1",
                "POST /v1/customers/cus_1 name=Kate+Live&email=&phone=&address=&shipping=&invoice_settings%5Bdefault_payment_method%5D=",
                "POST /v1/subscriptions/sub_1 default_payment_method=",
            ]
        );
    }
}
