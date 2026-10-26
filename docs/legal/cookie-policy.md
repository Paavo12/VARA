# FUber Cookie Policy

> **DRAFT — not yet in force.** For review by Namibian legal counsel. Items in `[brackets]` must be completed.

**Effective date:** [date] · **Version:** [2026-xx-xx]

## Is a cookie policy needed?

- **Mobile apps (Rider, Driver):** they do not use cookies. Sign-in tokens are kept in your phone's secure storage (Keychain on iPhone, Keystore on Android). This is covered by the [Privacy Policy](privacy-policy.md).
- **Websites (fuber.app and the Ops console):** they use a small number of cookies, so this policy explains them. At launch we use **only strictly necessary cookies**, so **no cookie consent banner is shown**. If we ever add analytics or other optional cookies, we will ask for your consent first and update this policy.

## 1. What cookies are

Cookies are small text files a website stores in your browser. Similar technologies include local storage. "Cookies" below covers both.

## 2. Cookies we use

### Strictly necessary (always on)

| Name | Site | Purpose | Duration |
| --- | --- | --- | --- |
| `fuber_ops_session` | Ops console (staff only) | Keeps FUber staff signed in securely (`HttpOnly`, `Secure`, `SameSite=Strict`) | 8 hours |
| `fuber_csrf` | Ops console, driver application form | Protects forms against cross-site request forgery | Session |
| `fuber_apply_draft` (local storage) | fuber.app driver application | Saves your unfinished application on your own device so you don't lose it | Until submitted or 7 days |
| `[captcha provider cookie]` | Sign-up and application forms | Distinguishes people from bots to prevent abuse | [per provider] |

The **trip-share page** (`fuber.app/t/…`) sets no cookies.

### Optional (not used at launch)

None. If we introduce privacy-friendly analytics in future, we will list each cookie here, explain it, and set it only after you agree. You will be able to change your choice at any time from the *Cookie settings* link in the website footer.

## 3. Third parties

The map on our web pages loads map tiles from **[map provider]**. Loading tiles sends your IP address and browser information to that provider, as with any web request; the provider does not set cookies for FUber **[confirm with provider]**. We do not use advertising or social-media tracking cookies.

## 4. Managing cookies

You can block or delete cookies in your browser settings. If you block strictly necessary cookies, parts of the website (such as the Ops console or the driver application form) will not work.

## 5. Contact

Questions: **[privacy@fuber.app]** · **[Company legal name]**, **[address]**, Windhoek, Namibia.
