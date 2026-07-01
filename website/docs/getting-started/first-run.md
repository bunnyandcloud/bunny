---
sidebar_position: 5
---

# First run

After installing bunny, follow these steps once per server or container.

## 1. Create a Discord bot (recommended)

Bunny integrates with Discord for threads, slash commands, and approvals.

1. Create an application in the [Discord Developer Portal](https://discord.com/developers/applications).
2. Add a bot and copy the token.
3. Invite the bot to your server.

Full walkthrough: [Discord application and server setup](../team-chats/discord/setup#discord-application-and-server).

You can skip Discord during `bunny configure` and run `bunny discord setup` later.

## 2. Configure the agent

See **[Configure the server](./configure-server)** for the full guide (config files, network, systemd).

```bash
bunny configure
```

This creates the **owner account**, sets up **MFA** (recommended), and optionally runs Discord setup.

Inside Docker, run via exec:

```bash
docker compose exec -it bunny bunny configure
```

## 3. Start the agent

```bash
bunny run
```

Inside Docker the agent must listen on all interfaces:

```bash
bunny run --host 0.0.0.0 --port 7681
```

Or set `BUNNY_SERVER__BIND_HOST=0.0.0.0` in your compose file.

## 4. Connect from your laptop

### Remote server or container (recommended)

When bunny runs on a **VPS or managed container**, open it with the **server's IP or URL** — not `127.0.0.1` on your laptop:

```text
http://203.0.113.5:7681          # public IP
http://bunny.internal:7681       # hostname on your LAN/VPN
https://your-host.example.com    # reverse proxy (recommended in production)
```

Requirements:

1. Bunny listens on `0.0.0.0` (step 3 — required in Docker).
2. Port `7681` is published and reachable (`7681:7681` in compose, firewall open if needed).
3. **`discord.public_url`** in `~/.config/bunny/config.yaml` matches that same base URL.

```yaml
discord:
  public_url: "https://your-host.example.com"   # or http://203.0.113.5:7681
```

`bunny configure` / `bunny discord setup` ask for this URL. You can also set `BUNNY_PUBLIC_URL`.

This matters for **Discord watch links**: `/bunny stream_browser_start` posts URLs like `{public_url}/watch/<token>`. If `public_url` is `http://127.0.0.1:7681`, only someone with an SSH tunnel on that machine can open them — teammates clicking the link in Discord will not reach the stream.

Use MFA and prefer HTTPS in production. See [Install on Linux](./install-linux#expose-on-public-ip).

### Local trial (Docker on your laptop)

When the container runs on the **same machine** as your browser:

```text
http://127.0.0.1:7681
```

`public_url` can stay `http://127.0.0.1:7681` for local Discord dev.

### SSH tunnel (solo admin access only)

Use when bunny (or Docker's port publish) listens only on **localhost of the remote host** and you do **not** want port 7681 on the network. Fine for your own Web UI access; **not** for shared Discord streams or OAuth.

```bash
ssh -L 7681:127.0.0.1:7681 user@your-server
```

Then open **http://127.0.0.1:7681** on your laptop while the tunnel is active.

```
  Laptop                    Remote server
  ┌─────────┐   SSH tunnel   ┌──────────────────┐
  │ Browser │ ─────────────► │ bunny :7681      │
  │ :7681   │   -L 7681:...  │ (127.0.0.1)      │
  └─────────┘                └──────────────────┘
```

## 5. Verify

```bash
bunny doctor
```

Checks Chromium, Xvfb, Node, sidecars, web UI, tmux, and git.

## 6. Secrets vault (optional)

For API keys and credentials injected into terminals:

```bash
bunny secrets init
bunny secrets set OPENAI_API_KEY --scope system
export BUNNY_SECRETS_PASSPHRASE='your-vault-passphrase'
```

See [Security](../security/) for scopes and CLI reference.

## Troubleshooting

| Problem | Fix |
|---------|-----|
| UI not loading | Check `public_url`, port mapping `7681`, firewall |
| Discord watch link broken | Set `discord.public_url` to a URL reachable from browsers (not `127.0.0.1` on a remote host) |
| Browser tab missing | Run full install (not `--minimal`); check `bunny doctor` |
| Discord bridge fails | Verify bot token; check outbound DNS from container |
