const STREAM_KEY_URL =
  /(\b(?:rtmps?|srt|rist):\/\/[^\s/?#]+(?:\/[^\s/?#]+)*\/)([^/\s?#]+)(?=[?#\s]|$)/gi;
const SENSITIVE_QUERY_VALUE =
  /([?&](?:key|token|passphrase|streamid|pass|password|secret|auth)=)[^&#\s]*/gi;
const BEARER_TOKEN = /\bBearer\s+[A-Za-z0-9._~+/-]+=*/gi;
const MASK = "••••";

export function redactLog(content: string): string {
  return content
    .replace(STREAM_KEY_URL, `$1${MASK}`)
    .replace(SENSITIVE_QUERY_VALUE, `$1${MASK}`)
    .replace(BEARER_TOKEN, `Bearer ${MASK}`);
}
