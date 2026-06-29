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

By default the agent binds to **localhost on the server**. Use an SSH tunnel:

```bash
ssh -L 7681:127.0.0.1:7681 user@your-server
```

Open **http://127.0.0.1:7681** on your laptop.

```
  Laptop                    Server / container
  ┌─────────┐   SSH tunnel   ┌──────────────────┐
  │ Browser │ ─────────────► │ bunny :7681      │
  │ :7681   │   -L 7681:...  │ (127.0.0.1)      │
  └─────────┘                └──────────────────┘
```

To expose the UI on the server's public IP (less secure), bind `0.0.0.0` and open the firewall. See [Install on Linux](./install-linux#expose-on-public-ip).

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
| UI not loading | Check tunnel or port mapping `7681` |
| Browser tab missing | Run full install (not `--minimal`); check `bunny doctor` |
| Discord bridge fails | Verify bot token; check outbound DNS from container |
