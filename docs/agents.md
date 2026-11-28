# The agent endpoint

Zeus can listen for a model harness — Claude Code and anything else that speaks MCP — on
`127.0.0.1`, and answer it with a grant the user wrote. This page is the user's guide: how to turn it
on, how to connect a harness, what an agent can do, and what the app does about it.

The endpoint is **the user's, in both directions**: no tool starts, stops or reconfigures it, and the
switch is on the Agents page, in the status bar, and on `Ctrl+Shift+A`.

## 1. Turning it on

Settings → Agents.

1. **New agent** mints a credential and shows its token **once**. Zeus keeps only its hash; a lost
   token is rotated, not recovered.
2. **Grant** what that credential may do. The grant has two halves:
   - the **wallets** it may act for, each with the capabilities it holds and the tokens and NFTs that
     may leave it — the allowlist is the bound, and an empty one allows nothing;
   - the **screens** it may read or write in Settings.
3. **Start server**. The status bar says which port it is on, and whether an agent is talking to it.

Nothing is granted by default. A fresh credential may ask `zeus_status`, `zeus_help` and
`agent_activity`, and it can see the four `settings_*` tools — but those refuse every screen until the
grant names it, and the refusal says which switch to turn on. No wallet is visible, and no write exists.

## 2. Connecting a harness

The token is the whole credential. The Agents page's **Connect** panel offers the snippet for the
harness in front of you, with the port filled in while the server is up, and **Test connection**
performs the real handshake from inside Zeus.

| Harness | Transport | What to paste |
|---|---|---|
| Claude Code | HTTP | `claude mcp add --transport http zeus http://127.0.0.1:<port>/mcp --header "Authorization: Bearer <token>"` |
| Any harness with a JSON config | HTTP | `{"mcpServers": {"zeus": {"type": "http", "url": "http://127.0.0.1:<port>/mcp", "headers": {"Authorization": "Bearer <token>"}}}}` |
| Any harness that asks for a URL and a header | HTTP | the URL `http://127.0.0.1:<port>/mcp`, and the header `Authorization: Bearer <token>` |

Adding a harness to that table is one row in `src/gui/ui/settings/agents/connect.rs`, not a new
screen. A token is a credential: treat it like a password for the wallet's grant, and rotate it from
the page if it leaks.

## 3. What an agent can do

Every tool is either a **read** or a **write**, and every write needs a capability the grant holds.
`zeus_help` is the authoritative list for the build in front of you — the names, the capability each
needs, and the meaning of every refusal code; `zeus_status` says what the calling credential holds.

- **Reads**: the wallet list, one wallet, its portfolio, private balances, transaction history,
  approvals, delegation, contacts, networks, pools, connected dapps, the settings screens, a swap
  quote — and `agent_activity`, which answers what this credential has done lately.
- **Writes**: sending a coin or token, sending an NFT, revoking an approval, delegating and
  undelegating, swapping, bridging, shielding, unshielding, merging private notes, adding a pool,
  adding or removing a contact, disconnecting a dapp, changing an endpoint, writing a settings
  screen, exporting data, deriving or importing a wallet.

A **write that moves money** runs the app's own screens:
the grant's allowlist, the recipient screen, the wallet lock, the app's confirmation window when the
profile asks for every change, and the app's record. A granted write with no prompt is announced out
loud and written to the activity ring.

Two ceilings bound an autonomous action, both read from the grant and both measured against what the
tool was promised rather than against a price:

- **Max price impact** (5% by default) — a swap whose simulated output is worth this much less than its
  quote is refused with `PRICE_IMPACT`.
- **Max shortfall** (1% by default) — an action that delivers this much less than the amount it promised
  is refused with `OUTPUT_SHORTFALL`. This is the one that catches a transfer tax: the simulation
  measures what the recipient actually receives, not what the calldata asked for.

A token Zeus has no dollar price for cannot have its impact measured, and an action whose output is
unpriced is refused with `PRICE_UNAVAILABLE` rather than guessed at. A bridge's own fee is not part of
the comparison: it is the bargain rather than a loss, so it comes off the quote before the two are
measured. All three refusals travel as `SIMULATION_REFUSED` over MCP, and the message carries the
numbers: "the trade loses 5.01% to price impact where this grant allows 5.00%". Both ceilings live in
the grant editor, and a profile set to ask before every action is reviewed by the person answering the
prompt, so none of the three fires there.

Every money answer carries the reading beside the diffs, whether it was allowed or refused:

    "review": {
      "price_impact_bps": 29,      // what the trade lost against its quote
      "shortfall_bps": 0,          // what arrived short of what was promised
      "fee_bps": null,             // a bridge's own cut, excluded from the comparison
      "usd_in": 1700.0, "usd_out": 1695.0,
      "unpriced": [],              // currencies nobody could price
      "measured": true, "complete": true
    }

A flow that states no promise — a dapp request, an approval — reports `null` rather than a row of
zeroes: "nothing was measured" and "nothing was lost" are different answers. The activity entry keeps
the same reading in its `detail`, because an autonomous action has no window anyone saw it in.

Amounts are decimal strings with the token's own decimals (`"1.25"`, `decimals: 6`), never JSON
numbers. Addresses are full and lowercased; a name never replaces one in a field an action consumes.
`action_id` makes a call idempotent: the same id with the same arguments replays the first answer
instead of sending twice.

## 4. What it says, and what it records

An autonomous action leaves two traces, because a user may not be watching the screen:

- **The announcement.** One sentence through the accessibility channel — "Agent sent 12 USDC to
  0x1234…abcd" — and, when something is refused, "Agent was denied: …". Sentences from tools wait their
  turn, so two actions finishing together are two announcements rather than one.
- **The activity ring.** Every call that ran and every refusal, with the tool, the wallet, the
  decision, the refusal's code, the transaction hash and the estimated value. A *read* is recorded too:
  being shown only the refusals made a read that ran look like a call that never happened. The cost is
  that a portfolio poll every thirty seconds is a row every thirty seconds, and the ring keeps the newest
  five thousand — a chatty harness spends history, never correctness, and a call is recorded once, so a
  write the pipeline broadcast is one row and not two. The Agents page shows it under **Activity**, newest
  first, and the `agent_activity` tool reads the same rows back to the agent that made them — never
  another credential's.

A row that produced a transaction links to the transaction history, where the outcome is.

## 5. Stopping it

- **Stop server** (the Agents page and the status bar, `Ctrl+Shift+A` from anywhere, including over a
  dialog): the listener closes, a request in flight fails with "the agent server was stopped", and
  **nothing is revoked** — the grants are still there, so Start resumes where it left off.
- **Disconnect all agents** (the status bar, no dialog in the way): the server stops **and** every
  credential is turned off, so no agent can reconnect until the user turns one back on. This is the
  one to press when something looks wrong.

Both are announced. A control whose only feedback is a colour is useless to the person this exists
for.

## 6. What this is not

- It is **not** a way to let someone else spend your money. The allowlist and the grant are the bound,
  and an agent cannot widen them.
- It is **not** protection from a process running as you. A local program that can run as this user
  can read the same files Zeus can; the endpoint raises the *cost* of misuse and leaves a trace, and
  says so plainly rather than pretending otherwise.
- It is **not** an approval surface. An agent can never approve a new dapp connection, type a
  password, or import a key: those are handed back to the user's own window, and the agent is told to
  wait for it.

What that means in practice: the endpoint is worth turning on because it bounds what a model can do
and says what it did, and it is not worth turning on for a wallet you would not leave unlocked on a
shared machine.
