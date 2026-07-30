export const serverCsrfCookieName = "__Host-qingyu_csrf";

const csrfTokenPattern = /^[A-Za-z0-9_-]+$/u;

/**
 * Reads the public double-submit cookie without decoding or normalizing it.
 * Any ambiguity in the target cookie fails closed before a mutation is sent.
 */
export function readServerCsrfCookie(cookieHeader: string): string | null {
  let csrfToken: string | null = null;

  for (const segment of cookieHeader.split(";")) {
    const cookie = segment.trim();
    if (cookie === "") continue;
    const separator = cookie.indexOf("=");
    if (separator < 1) continue;
    const name = cookie.slice(0, separator);
    if (name !== serverCsrfCookieName) continue;
    const value = cookie.slice(separator + 1);
    if (csrfToken !== null || !csrfTokenPattern.test(value)) return null;
    csrfToken = value;
  }

  return csrfToken;
}
