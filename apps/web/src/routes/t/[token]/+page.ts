// The page asks the API from the visitor's browser, so the API's per-IP limit counts each
// visitor (TRD §7.1), and the token never passes through this server's logs.
export const ssr = false;
export const prerender = false;
