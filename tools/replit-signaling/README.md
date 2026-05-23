# Matchbox signaling server for Starcon — Replit setup

This folder is a paste-ready Replit config for running a public
`matchbox_server` so you can play Starcon online with friends.
GitHub Pages is static-only and can't host the WebSocket
signaling server itself; this Repl provides the missing piece.

## One-time setup

1. On https://replit.com, **Create Repl** → choose **Blank Repl**
   (template doesn't matter; we override it via config).

2. In your new Repl, open the file tree and **replace the
   contents** of these files with the files from this folder
   (create them if they don't exist):
   - `.replit`
   - `replit.nix`
   - `main.sh`

3. **In the workspace**, click **Run**. First run takes ~5 min
   to compile `matchbox_server` into `./bin/`; subsequent runs
   start in seconds. Verify it prints
   `Starting matchbox_server on 0.0.0.0:3536` and the Webview
   shows a URL.

4. **Deploy as Reserved VM** (the only deployment type that
   keeps WebSockets alive — Autoscale's session-affinity
   gotchas break matchbox):
   - Click **Deploy** (top-right).
   - Pick **Reserved VM**, smallest tier.
   - Hit **Deploy**. The build step (`cargo install`) takes
     ~5 minutes (no port-open health-check during build).
   - Once deployed you'll get a URL like
     `https://your-deploy-name.replit.app`.

## Using it in the game

Both you and your friend visit the game URL with the signaling
server appended (`wss://`, not `https://`):

```
https://johngcarlsson-usc.github.io/starcon/?signal=wss://your-deploy-name.replit.app
```

Both click "Online", set the **same humans/AI counts**, click
"Find Match". The lobby will show "Waiting for players: 1/2"
on each peer until both connect, then drop into the match.

## Troubleshooting

- **"Waiting for players: 1/2" forever, both peers**: usually
  Autoscale routing. Each WebSocket connection lands on a
  different stateless instance, so matchbox's in-memory waiting
  room never sees both peers. Fix: redeploy as Reserved VM.
- **"Waiting for players: 1/2" on one peer only**: the peers
  picked different humans/AI mixes in the lobby setup. The
  room URL encodes the combo (`h2-a0` vs `h2-a1`) so they
  end up in different matchbox rooms. Pick the same combo
  on both ends.
- **Deployment dies during build**: usually means the Reserved
  VM tier you picked has too little RAM for the Rust compile.
  Try a larger tier, or pre-compile in the workspace first
  (`./bin/matchbox_server` should exist before deploying;
  the snapshot will include it).
- **`https://...replit.app/` returns 500 / Internal Server Error**:
  matchbox_server isn't running. Check the deployment logs;
  the binary might not have started, or it might still be
  compiling if the build step was skipped.
