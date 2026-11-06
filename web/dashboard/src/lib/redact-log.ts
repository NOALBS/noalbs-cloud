const STREAM_KEY_URL =
  /(\b(?:rtmps?|srt|rist):\/\/[^\s/?#]+(?:\/[^\s/?#]+)*\/)([^/\s?#]+)(?=[?#\s]|$)/gi;
const SENSITIVE_QUERY_VALUE =
  /([?&](?:key|token|passphrase|streamid|pass|password|secret|auth)=)[^&#\s]*/gi;
const BEARER_TOKEN = /\bBearer\s+[A-Za-z0-9._~+/-]+=*/gi;
const IPV4_ADDRESS = /(?<![\d.])(?:\d{1,3}\.){3}\d{1,3}(?![\d.])/g;
const IPV6_CANDIDATE =
  /(?<![0-9a-f:.])(?:[0-9a-f]{0,4}:){2,7}(?:[0-9a-f]{0,4}|(?:\d{1,3}\.){3}\d{1,3})(?![0-9a-f:.])/gi;
const MASK = "••••";

function isIpv6Address(candidate: string): boolean {
  try {
    new URL(`http://[${candidate}]/`);
    return true;
  } catch {
    return false;
  }
}

export function redactLog(content: string): string {
  return content
    .replace(STREAM_KEY_URL, `$1${MASK}`)
    .replace(SENSITIVE_QUERY_VALUE, `$1${MASK}`)
    .replace(BEARER_TOKEN, `Bearer ${MASK}`)
    .replace(IPV6_CANDIDATE, (candidate) => (isIpv6Address(candidate) ? MASK : candidate))
    .replace(IPV4_ADDRESS, MASK);
}
