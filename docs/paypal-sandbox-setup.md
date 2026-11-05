# PayPal sandbox setup for AutoFIRC

Everything AutoFIRC does against PayPal works in the free sandbox. This guide
takes ~10 minutes. References: [sandbox testing guide](https://developer.paypal.com/api/rest/sandbox/),
[checkout webhooks](https://developer.paypal.com/payment-methods/webhooks),
[PayPal MCP quickstart](https://docs.paypal.ai/developer/tools/ai/mcp-quickstart).

## 1. Developer account + app (5 min)

1. Log in at <https://developer.paypal.com/> (any PayPal account works).
2. **Dashboard → Apps & Credentials → Create App** (sandbox). Name it e.g.
   `autofirc`.
3. Copy the **Client ID** and **Secret** into `.env`:

   ```
   PAYPAL_BASE_URL=https://api-m.sandbox.paypal.com
   PAYPAL_CLIENT_ID=<client id>
   PAYPAL_CLIENT_SECRET=<secret>
   ```

4. Under **Sandbox → Accounts** note the auto-created accounts: a *Business*
   account (this is the merchant receiving money — your app's identity) and a
   *Personal* account (the buyer you'll log in as on the checkout page).

## 2. Webhook subscription

The pipeline is driven by `PAYMENT.CAPTURE.COMPLETED` webhooks.

1. Expose your server publicly. Local dev tunnel (pick one):

   ```bash
   ssh -R 80:localhost:8080 nokey@localhost.run   # free, no signup
   # or: ngrok http 8080
   ```

   Deployed: use the Render URL from `render.yaml`.

2. **Dashboard → Apps & Credentials → your app → Webhooks → Add Webhook**:

   - URL: `https://<your-tunnel-or-render-host>/api/webhooks/paypal`
   - Events: `PAYMENT.CAPTURE.COMPLETED` (add `CHECKOUT.ORDER.APPROVED`
     if you want the earlier signal too — AutoFIRC only needs the capture).

3. Copy the **Webhook ID** into `.env` as `PAYPAL_WEBHOOK_ID`. AutoFIRC
   verifies every delivery (CRC32 + RSA-SHA256 against PayPal's cert, with an
   https-only `*.paypal.com` cert-URL allowlist) — a wrong or missing ID
   fails closed with 401.

4. Sanity-check with the [Webhook Simulator](https://developer.paypal.com/dashboard/webhooksSimulator):
   send a `PAYMENT.CAPTURE.COMPLETED` to your URL; expect HTTP 200 and a new
   row (`autofirc list`). Simulator events carry synthetic ids, so the enrich
   step will fail to fetch the order — use it only to verify signature
   handling and ingestion. Real flow comes from step 3 below.

## 3. End-to-end: a real sandbox payment

1. `unset DEV_WEBHOOK_KEY_PEM` (real cert verification now applies).
2. Start the stack: `./scripts/sandbox-demo.sh`
3. Open `http://localhost:8080/checkout`, pick a service/amount, pay with the
   **Personal sandbox account** credentials.
4. Watch the payment traverse the pipeline:

   ```bash
   autofirc watch        # or the dashboard at http://localhost:5173
   ```

   webhook → enrich (real order/capture data, real INR settlement estimate)
   → AI classification → GST invoice + LUT PDFs → `compliance_packet_ready`.
5. Petition + reconcile as in the offline demo (`autofirc petition …`).

## 4. PayPal MCP server (agentic commerce)

PayPal's official MCP server exposes orders, invoices, subscriptions, and
disputes as agent tools. This repo ships a ready config (`.mcp.json.example`
at the root — rename it to `.mcp.json` to activate) pointing at the sandbox
endpoint — open it with any MCP client (Claude Code, Kimi, Cursor) and
complete the PayPal login/consent flow. Note: activating it makes the client
open PayPal's OAuth consent page in your browser on every session until you
complete the login once — rename it back if you don't want that.

```
npx mcp-remote https://mcp.sandbox.paypal.com/sse
```

Or run it locally with a static token:

```json
{
  "mcpServers": {
    "paypal": {
      "command": "npx",
      "args": ["-y", "@paypal/mcp", "--tools=all"],
      "env": {
        "PAYPAL_ACCESS_TOKEN": "<sandbox access token>",
        "PAYPAL_ENVIRONMENT": "SANDBOX"
      }
    }
  }
}
```

Get a sandbox access token with:

```bash
curl -u "$PAYPAL_CLIENT_ID:$PAYPAL_CLIENT_SECRET" \
  -d grant_type=client_credentials \
  https://api-m.sandbox.paypal.com/v1/oauth2/token
```

With both `autofirc` (this repo's CLI/skill) and PayPal's MCP tools loaded,
an agent can answer "which Globex payment still needs its eBRC, and was the
invoice sent?" end-to-end across compliance and payment state.
