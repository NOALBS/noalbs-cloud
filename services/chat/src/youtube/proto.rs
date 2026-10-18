//! The messages of YouTube's `StreamList` RPC that we read, from its
//! `stream_list.proto` (vendored in `proto/` for reference). Written by hand so
//! the build needs no protoc; prost skips the fields left out.

/// The RPC's path (`package.Service/Method`).
pub const STREAM_LIST: &str = "/youtube.api.v3.V3DataLiveChatMessageService/StreamList";

/// `LiveChatMessageSnippet.TypeWrapper.Type` values we act on.
pub const TEXT_MESSAGE_EVENT: i32 = 1;
pub const CHAT_ENDED_EVENT: i32 = 4;

#[derive(Clone, PartialEq, prost::Message)]
pub struct LiveChatMessageListRequest {
    #[prost(string, optional, tag = "1")]
    pub live_chat_id: Option<String>,
    #[prost(string, optional, tag = "99")]
    pub page_token: Option<String>,
    #[prost(string, repeated, tag = "100")]
    pub part: Vec<String>,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct LiveChatMessageListResponse {
    /// When the stream went offline (ISO 8601); the chat is over.
    #[prost(string, optional, tag = "2")]
    pub offline_at: Option<String>,
    #[prost(string, optional, tag = "100602")]
    pub next_page_token: Option<String>,
    #[prost(message, repeated, tag = "1007")]
    pub items: Vec<LiveChatMessage>,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct LiveChatMessage {
    #[prost(string, optional, tag = "101")]
    pub id: Option<String>,
    #[prost(message, optional, tag = "2")]
    pub snippet: Option<LiveChatMessageSnippet>,
    #[prost(message, optional, tag = "3")]
    pub author_details: Option<LiveChatMessageAuthorDetails>,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct LiveChatMessageSnippet {
    #[prost(int32, optional, tag = "1")]
    pub r#type: Option<i32>,
    /// ISO 8601.
    #[prost(string, optional, tag = "4")]
    pub published_at: Option<String>,
    #[prost(message, optional, tag = "19")]
    pub text_message_details: Option<LiveChatTextMessageDetails>,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct LiveChatTextMessageDetails {
    #[prost(string, optional, tag = "1")]
    pub message_text: Option<String>,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct LiveChatMessageAuthorDetails {
    #[prost(string, optional, tag = "10101")]
    pub channel_id: Option<String>,
    /// The handle, with its `@`, for most channels.
    #[prost(string, optional, tag = "103")]
    pub display_name: Option<String>,
    #[prost(bool, optional, tag = "5")]
    pub is_chat_owner: Option<bool>,
    /// A member of the channel.
    #[prost(bool, optional, tag = "6")]
    pub is_chat_sponsor: Option<bool>,
    #[prost(bool, optional, tag = "7")]
    pub is_chat_moderator: Option<bool>,
}

#[cfg(test)]
mod tests {
    use prost::Message;

    use super::*;

    /// Bytes as YouTube would send them, with fields we don't read in between
    /// (`kind` = 200, `etag` = 201, `is_verified` = 4, `display_message` = 16).
    #[test]
    fn decodes_the_fields_we_read_and_skips_the_rest() {
        // Tag bytes for field numbers above 15 are multi-byte varints; build
        // them with prost's own encoder to keep the test readable.
        use prost::encoding::{bool, int32, string};
        let mut author = Vec::new();
        string::encode(10101, &"UCchatter".to_owned(), &mut author);
        string::encode(103, &"@viewer".to_owned(), &mut author);
        bool::encode(4, &true, &mut author);
        bool::encode(7, &true, &mut author);
        let mut details = Vec::new();
        string::encode(1, &"!bitrate".to_owned(), &mut details);
        let mut snippet = Vec::new();
        int32::encode(1, &TEXT_MESSAGE_EVENT, &mut snippet);
        string::encode(4, &"2026-09-29T12:00:00Z".to_owned(), &mut snippet);
        string::encode(16, &"!bitrate".to_owned(), &mut snippet);
        prost::encoding::bytes::encode(19, &details, &mut snippet);
        let mut item = Vec::new();
        string::encode(200, &"youtube#liveChatMessage".to_owned(), &mut item);
        string::encode(101, &"msg-1".to_owned(), &mut item);
        prost::encoding::bytes::encode(2, &snippet, &mut item);
        prost::encoding::bytes::encode(3, &author, &mut item);
        let mut response = Vec::new();
        string::encode(201, &"etag".to_owned(), &mut response);
        string::encode(100602, &"next".to_owned(), &mut response);
        prost::encoding::bytes::encode(1007, &item, &mut response);

        let decoded = LiveChatMessageListResponse::decode(response.as_slice()).unwrap();
        assert_eq!(decoded.next_page_token.as_deref(), Some("next"));
        assert_eq!(decoded.offline_at, None);
        let item = &decoded.items[0];
        assert_eq!(item.id.as_deref(), Some("msg-1"));
        let snippet = item.snippet.as_ref().unwrap();
        assert_eq!(snippet.r#type, Some(TEXT_MESSAGE_EVENT));
        assert_eq!(
            snippet
                .text_message_details
                .as_ref()
                .and_then(|details| details.message_text.as_deref()),
            Some("!bitrate")
        );
        let author = item.author_details.as_ref().unwrap();
        assert_eq!(author.channel_id.as_deref(), Some("UCchatter"));
        assert_eq!(author.display_name.as_deref(), Some("@viewer"));
        assert_eq!(author.is_chat_moderator, Some(true));
        assert_eq!(author.is_chat_owner, None);
    }

    #[test]
    fn the_request_uses_youtubes_field_numbers() {
        let request = LiveChatMessageListRequest {
            live_chat_id: Some("chat".into()),
            page_token: Some("page".into()),
            part: vec!["id".into()],
        };
        let mut expected = Vec::new();
        prost::encoding::string::encode(1, &"chat".to_owned(), &mut expected);
        prost::encoding::string::encode(99, &"page".to_owned(), &mut expected);
        prost::encoding::string::encode(100, &"id".to_owned(), &mut expected);
        assert_eq!(request.encode_to_vec(), expected);
    }
}
