# Decision 23: Request checks: JSON for every mutating request

**Every mutating request (any method except `GET`, `HEAD`, `OPTIONS`) must be `Content-Type: application/json`, whether or not it carries `Origin`** (recorded 2026-11-08, §10.1). The `Origin` match applies only when `Origin` is present. Why: with auth off, a cross-site `text/plain` form post can set hooks and `password-command`, and a browser or extension that strips `Origin` would otherwise let it through unchecked. Cost: curl and scripts must send the header.
