# Matchbox signaling server for Starcon — Replit setup

This folder is a paste-ready Replit config for running a public
`matchbox_server` so you can play Starcon online with friends.
GitHub Pages is static-only and can't host the WebSocket
signaling server itself; this Repl provides the missing piece.

## One-time setup (~10 minutes)

1. On https://replit.com, **Create Repl** → choose **Blank Repl**
   (template doesn't matter; we override it via config).

2. In your new Repl, open the file tree and **replace the
   contents** of these files with the files from this folder
   (create them if they don't exist):
   - `.replit`
   - `replit.nix`
   - `main.sh`

3. Click **Run** in Replit. The first run will take ~5 minutes
   while it downloads + compiles `matchbox_server`. Subsequent
   runs start in seconds (cached in your Repl's persistent
   volume).

4. Once it's running, the Replit webview shows
   `Matchbox Signaling Server: 0.0.0.0:3536` and the Webview
   tab gives you a URL like
   `https://your-repl-name.your-username.repl.co`.

5. **Keep the Repl Always-On** (you have $20/month Pro, so
   this is included): in your Repl, click the Repl name in the
   top-left → **Always On** → toggle on. Otherwise the Repl
   will sleep after ~5 minutes of inactivity and your friend
   won't be able to connect.

## Using it in the game

Both you and your friend visit the game URL with the signaling
server appended:

```
https://johngcarlsson-usc.github.io/starcon/?signal=wss://your-repl-name.your-username.repl.co
```

(Note the `wss://` — TLS-secured WebSocket, NOT `https://` —
matchbox uses raw WebSockets through Replit's TLS proxy.)

Both click "Online", set the same humans/AI counts, click
"Find Match". The lobby will show "Waiting for players: 1/2"
on each peer until both connect, then drop into the match.

## Troubleshooting

- **"Waiting for players: 1/2" forever**: check that both peers
  used the exact same `?signal=...` URL. Different signaling
  servers don't share rooms.
- **"Connection failed: ..."**: check the Repl is running.
  Replit might have put it to sleep — toggle Always-On.
- **Replit URL format isn't `repl.co`**: newer Repls might give
  you a `.replit.dev` or similar URL. Use whatever URL is shown
  in the Webview tab; just replace `https://` with `wss://`.
