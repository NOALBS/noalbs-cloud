//! The bot's translations: `locales/<language>/messages.po`, the format of the
//! dashboard's Lingui catalogs. Messages are written in English with [`tr!`] in
//! `messages.rs` and that English is the key, so a language without a translation
//! for a message says it in English.
//!
//! After changing what the bot says, add the change to the catalogs (a test fails
//! until they match): `UPDATE_CATALOGS=1 cargo test -p chat catalogs`.

use std::{
    collections::HashMap,
    fmt::{Display, Write},
    sync::LazyLock,
};

use polib::po_file;

/// English has none: it's what the messages are written in.
const CATALOGS: &[(&str, &str)] = &[
    ("de", include_str!("../locales/de/messages.po")),
    ("es", include_str!("../locales/es/messages.po")),
    ("fr", include_str!("../locales/fr/messages.po")),
    ("it", include_str!("../locales/it/messages.po")),
    ("nl", include_str!("../locales/nl/messages.po")),
    ("pl", include_str!("../locales/pl/messages.po")),
    ("pt-br", include_str!("../locales/pt-br/messages.po")),
    ("sv", include_str!("../locales/sv/messages.po")),
    ("tr", include_str!("../locales/tr/messages.po")),
    ("zh-tw", include_str!("../locales/zh-tw/messages.po")),
];

type Catalog = HashMap<String, String>;

static TRANSLATIONS: LazyLock<HashMap<&str, Catalog>> = LazyLock::new(|| {
    CATALOGS
        .iter()
        .map(|(language, catalog)| (*language, parse(catalog)))
        .collect()
});

/// Each message and its translation, empty while it has none.
fn parse(catalog: &str) -> Catalog {
    po_file::parse_from_reader(catalog.as_bytes())
        .expect("catalogs parse")
        .messages()
        .map(|message| {
            let translation = message.msgstr().expect("catalogs have no plurals");
            (message.msgid().to_owned(), translation.to_owned())
        })
        .collect()
}

/// A message in `language`, worded like `format!`: `tr!(lang, "Hello {name}", name = name)`.
/// The message must be a literal, for the catalogs to be checked against the code.
macro_rules! tr {
    ($language:expr, $source:literal $(, $name:ident = $value:expr)* $(,)?) => {
        $crate::i18n::translate(
            $language,
            $source,
            &[$((stringify!($name), &$value as &dyn std::fmt::Display)),*],
        )
    };
}
pub(crate) use tr;

pub fn translate(language: &str, source: &str, args: &[(&str, &dyn Display)]) -> String {
    let template = TRANSLATIONS
        .get(language)
        .and_then(|catalog| catalog.get(source))
        .filter(|translation| !translation.is_empty())
        .map_or(source, String::as_str);
    fill(template, args)
}

/// Replaces `{name}`s in one pass: a scene called `{name}` stays as it is.
fn fill(template: &str, args: &[(&str, &dyn Display)]) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let arg = after.split_once('}').and_then(|(name, tail)| {
            let (_, value) = args.iter().find(|(arg, _)| *arg == name)?;
            Some((value, tail))
        });
        match arg {
            Some((value, tail)) => {
                let _ = write!(out, "{value}");
                rest = tail;
            }
            None => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{CATALOGS, TRANSLATIONS, fill, parse};

    /// Every message in `messages.rs`, in order.
    fn sources() -> Vec<String> {
        let code = include_str!("messages.rs");
        let mut sources = Vec::new();
        for (at, _) in code.match_indices("tr!(") {
            let (_, literal) = code[at..].split_once('"').expect("tr! has a message");
            let mut source = String::new();
            let mut chars = literal.chars();
            loop {
                match chars.next().expect("the message ends") {
                    '"' => break,
                    '\\' => source.extend(chars.next()),
                    c => source.push(c),
                }
            }
            if !sources.contains(&source) {
                sources.push(source);
            }
        }
        sources
    }

    fn placeholders(text: &str) -> Vec<&str> {
        let mut found: Vec<&str> = text
            .split('{')
            .skip(1)
            .filter_map(|part| part.split_once('}').map(|(name, _)| name))
            .collect();
        found.sort_unstable();
        found
    }

    fn escape(text: &str) -> String {
        text.replace('\\', "\\\\").replace('"', "\\\"")
    }

    #[test]
    fn catalogs_match_the_code() {
        let sources = sources();
        assert!(!sources.is_empty());
        for (language, catalog) in CATALOGS {
            let parsed = parse(catalog);
            if std::env::var_os("UPDATE_CATALOGS").is_some() {
                let header = catalog.split_once("\n\n").map_or(*catalog, |(top, _)| top);
                let mut updated = format!("{}\n", header.trim_end());
                for source in &sources {
                    let translation = parsed.get(source).map_or("", String::as_str);
                    updated.push_str(&format!(
                        "\nmsgid \"{}\"\nmsgstr \"{}\"\n",
                        escape(source),
                        escape(translation)
                    ));
                }
                let path = format!(
                    "{}/locales/{language}/messages.po",
                    env!("CARGO_MANIFEST_DIR")
                );
                std::fs::write(path, updated).expect("catalogs are writable");
                continue;
            }
            let wanted: BTreeSet<&str> = sources.iter().map(String::as_str).collect();
            let found: BTreeSet<&str> = parsed.keys().map(String::as_str).collect();
            assert_eq!(
                found, wanted,
                "{language} is out of date: UPDATE_CATALOGS=1 cargo test -p chat catalogs"
            );
        }
    }

    #[test]
    fn translations_keep_the_placeholders() {
        for (language, catalog) in TRANSLATIONS.iter() {
            for (source, translation) in catalog {
                if !translation.is_empty() {
                    assert_eq!(
                        placeholders(translation),
                        placeholders(source),
                        "{language}: {translation}"
                    );
                }
            }
        }
    }

    #[test]
    fn every_language_has_a_catalog() {
        let catalogs: BTreeSet<&str> = CATALOGS.iter().map(|(language, _)| *language).collect();
        let languages: BTreeSet<&str> = db::chat_commands::LANGUAGES
            .iter()
            .copied()
            .filter(|language| *language != "en")
            .collect();
        assert_eq!(catalogs, languages);
    }

    #[test]
    fn filling_in_leaves_values_alone() {
        let name = "{scene}";
        assert_eq!(
            fill("{name} and {scene}", &[("name", &name), ("scene", &1)]),
            "{scene} and 1"
        );
        assert_eq!(fill("{nope} {", &[]), "{nope} {");
    }

    #[test]
    fn languages_without_a_translation_say_it_in_english() {
        assert_eq!(tr!("en", "Already streaming"), "Already streaming");
        assert_eq!(tr!("xx", "Already streaming"), "Already streaming");
        assert_eq!(tr!("de", "Not in any catalog"), "Not in any catalog");
    }
}
