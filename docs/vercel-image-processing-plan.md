# Image processing on Vercel

1. **Split:** `default` serves pages/auth/coordination; `images` handles codecs; `vision` contains the existing recognition pipeline and private native assets. Pin framework and CLI to `9fa4141d145f5afba869a42857e601c08a74e5fc`.
2. **Background:** persist the existing database job before notifying the worker through NextRS `WaitUntil`. Process one leased photo per invocation, renew leases, chain pending work, and cap retries at five. Keep manual identity corrections.
3. **Recovery:** declare a five-minute `#[nextrs::cron(..., provider = "cloudflare")]` route and deploy it with `nextrs cron deploy`. The generated timer calls Vercel; inference stays on Vercel. Retain the daily reconciliation cron.
4. **Verify:** test packaged adapters, live upload completion, landmarks/embeddings/matching, missed notification recovery, auth, and browser navigation. Report each function’s executable, libraries, models, and total size against an equivalent unsplit build.

Production deployment and existing pending-photo processing are authorized. Work is isolated on `codex/vercel-image-processing`; the original dirty checkout is preserved. Disable hosted dispatch and redeploy the previous version to roll back.
