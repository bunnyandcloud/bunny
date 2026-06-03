# Discord + Docker (dev Mac) — guide court

Tout se passe **dans le conteneur**. Sur ton Mac : Docker + les scripts `docker-dev.sh` seulement.

## Le plus simple (recommandé)

```bash
./scripts/docker-dev.sh bootstrap
```

Une seule commande : installe Rust (1ʳᵉ fois), compile, lance `bunny configure` (email, mot de passe, Discord).

Ensuite :

```bash
./scripts/docker-dev.sh shell
bunny run                    # terminal 1 — garder ouvert
```

Autre terminal :

```bash
./scripts/docker-dev.sh start-bridge
```

- Web UI : http://127.0.0.1:7681  
- Discord : `/bunny help` puis `/bunny link` (code dans la Web UI → session → **Discord**)

## Étapes manuelles (équivalent)

```bash
./scripts/docker-dev.sh up
./scripts/docker-dev.sh shell    # installe Rust automatiquement si besoin
bunny configure
bunny run                        # terminal 1
# terminal 2 :
bunny discord bridge
```

## Commandes `bunny`

| Commande | Rôle |
|----------|------|
| `bunny setup --minimal` | Installe Rust/outils (1ʳᵉ fois dans le conteneur) |
| `bunny configure` | Compte admin + option Discord |
| `bunny discord setup` | Config Discord seule |
| `bunny discord bridge` | Lance le bot Discord |
| `bunny run` | Agent + Web UI (:7681) |

## Scripts Mac

| Commande | Rôle |
|----------|------|
| `./scripts/docker-dev.sh bootstrap` | Install + `bunny configure` interactif |
| `./scripts/docker-dev.sh browser-setup` | Xvfb + Chromium + noVNC pour l’onglet Browser |
| `./scripts/docker-dev.sh shell` | Shell (auto `setup` si pas de Rust) |
| `./scripts/docker-dev.sh start-agent` | `bunny run` |
| `./scripts/docker-dev.sh start-bridge` | `bunny discord bridge` |
| `./scripts/docker-dev.sh check-network` | Test DNS/HTTPS vers Discord dans le conteneur |
| `./scripts/docker-dev.sh status` | Santé |
| `./scripts/docker-dev.sh down -v` | Reset complet |

## Dépannage

| Problème | Action |
|----------|--------|
| `Rust toolchain required` | `bunny setup --minimal` puis `bunny configure` |
| Page blanche sur :7681 | `bunny run` |
| Browser : `Xvfb` / `No such file` | `./scripts/docker-dev.sh browser-setup` (le `setup --minimal` n’installe pas la stack navigateur) |
| `/bunny` ne répond pas | `start-bridge` (1 terminal dédié). Arrêt : **Ctrl+C** dans ce terminal, ou `./scripts/docker-dev.sh stop-bridge` |
| `405 Method Not Allowed` sur `shell_close` / nouvelle commande | Rebuild + **redémarrer l’agent** : dans le conteneur `cargo build --release -p bunny-server`, puis Ctrl+C sur `bunny run` et relancer |
| `discord` inconnu | `bunny setup --minimal` (recompile le CLI) |
| `DisallowedGatewayIntents` | Portail Discord → ton bot → **Privileged Gateway Intents** → activer **Message Content Intent** (et enregistrer), puis relancer `start-bridge` |
| `failed to lookup address` / `HTTP request to get gateway URL failed` au lancement du bridge | **Réseau DNS du conteneur** (pas Discord) — avant toute action sur Discord. `./scripts/docker-dev.sh check-network` puis `down` + `up` pour appliquer le DNS du compose ; redémarrer Docker Desktop si ça persiste |
| `invalid bridge token` sur `/bunny link` | Le token dans `.discord/bridge.yaml` n’est pas dans la config agent — `bunny discord sync` puis **redémarrer `bunny run`** (Ctrl+C, relancer) |
| `discord account not linked to bunny user` sur `run` | Redémarrer `bunny run` (fix récent), puis retenter `/bunny run` — ou refaire `/bunny link` avec un nouveau code Web UI |
| `/bunny run` + `npm run dev` → timeout 40s | Rebuild `bunny-server` : `/bunny run` injecte dans tmux ; si la commande ne finit pas en ~8s, Discord indique **processus persistant** + extrait, logs dans l’onglet Terminal. |
| Arrêter `npm run dev` (ou autre) | `/bunny run_stop` — Ctrl+C sur le shell du canal (`shell:<nom>` si besoin). Pas `/bunny stop` (tâches Claude). |
| Choisir un shell | `/bunny shell_list` puis `/bunny run shell:<nom> command:pwd` (sans `shell:` = premier shell créé) |
| Créer un shell | `/bunny shell_new` ou `/bunny shell_new name:debug` |
| Fermer un shell | `/bunny shell_close shell:shell 1` (sans `shell:` si un seul onglet) |
| Snapshot shell | `/bunny snapshot` ou `/bunny snapshot shell:shell 1` — légende Discord indique le shell |
| Snapshot complet | `/bunny full_snapshot` — shell + browser (Chromium démarré auto sur :3000 ou 1er preview) |
| Stream browser | `/bunny stream_browser_start` — read-only par défaut ; `interactive:true` pour contrôle souris/clavier |
| Arrêter stream browser | `/bunny stream_browser_stop` — tous les liens actifs du canal ; `url:<watch URL>` pour un lien précis |
| Browser : écran noir en **Stream** / watch | Normal en Docker avec l’ancien WebRTC — rebuild Web UI + agent, puis Stream/watch passent par noVNC read-only (tunnel :7681). Relance `bunny run`. |
| Slash commands doublées (`run` + `shell_run`, chaque cmd x2) | **Global + guild** en parallèle. `./scripts/docker-dev.sh stop-bridge` puis **un seul** `start-bridge`. Vérifie `guild_id` dans `.discord/bridge.yaml`. Quitte Discord (Cmd+Q). Log attendu : `removed stale global slash commands`. |
| Mention `@bunny` → pas de réponse dans le thread | Rebuild + redémarrer agent + bridge : `cargo build --release -p bunny-server -p bunny-discord-bridge`, puis Ctrl+C sur `bunny run` et `start-bridge`. Les threads utilisent `claude -p` (headless) — la réponse arrive dans Discord après la fin de l’appel (jusqu’à ~5 min). |
| Thread : shell `discord-*` sans réponse Discord | Vérifier que Claude Code est installé (`?claude=setup` dans la Web UI). Le shell thread affiche le transcript des commandes `claude -p`, pas une session interactive. |
| Thread : typing puis silence (pas de GOAL) | Ancien bug : `claude -p` était bloqué comme commande « interactive ». Rebuild `bunny-server` + `bunny-discord-bridge`, redémarrer agent + bridge. |

### Threads Discord (`@bunny` mention)

- Mention du bot dans un canal → thread auto + shell `discord-*` dans la Web UI (cwd projet).
- Claude est invoqué en **headless** (`claude -p --output-format json`) ; la réponse est postée **directement** dans le thread (plus de polling tmux).
- Messages suivants (reply ou @mention) réinjectent le **Goal** (contexte) + l’historique du thread ; seul l’utilisateur clôture avec **Goal!** (Claude ne déclare pas l’objectif atteint).
- **Goal!** / **Cancel** ferment le shell thread (onglet disparaît de la Web UI).
- Réaction ⛔ sur le dernier message input → interruption du subprocess Claude en cours.
- Shell `discord-*` dans la Web UI : le transcript `[discord] $ claude -p …` apparaît après l’appel (recharger l’onglet si besoin).
- Erreur `error_max_turns` : le plan partiel est extrait du JSON si présent ; pour un plan sans choix, demander « écris le plan en markdown ».
- **AskUserQuestion** : si Claude a besoin d’un choix, le bot poste des **boutons** dans le thread ; après clic, `claude -p --resume` reprend avec vos réponses. Plusieurs questions → un message/boutons par question.

### Activer les intents Discord (obligatoire une fois)

1. [Discord Developer Portal](https://discord.com/developers/applications) → ton application → **Bot**
2. Section **Privileged Gateway Intents**
3. Active **Message Content Intent** (pour `/bunny` et les mentions `@bunny`)
4. **Save Changes**
5. Relance `./scripts/docker-dev.sh start-bridge` (avec `bunny run` déjà lancé dans l’autre terminal)

Guide complet : [discord.md](discord.md).
