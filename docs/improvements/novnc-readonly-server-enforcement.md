# Amélioration : enforcement read-only noVNC côté serveur

**Statut :** piste future (v1 acceptée côté client)  
**Contexte :** liens watch Discord (`/watch/:token`), onglet Stream read-only Web UI  
**Implémentation actuelle :** `apps/server/src/novnc_proxy.rs` (`NovncEmbedLock`, paramètre `bunny_lock`)

---

## Comportement actuel (v1)

Pour distinguer **interactif** et **read-only**, le serveur sert une page `vnc.html` modifiée :

| Mode | Mécanisme |
|------|-----------|
| **Interactif** | Script injecté : `localStorage.view_only = false`, case « Afficher uniquement » décochée au chargement |
| **Read-only** | Panneau Settings noVNC masqué (CSS), `localStorage.view_only = true`, case cochée + `disabled` + revert au `change` |

Sur les liens **watch**, le mode est dérivé de `watch.mode` en base (`interactive` vs `read_only`) — le query param `bunny_lock` envoyé par le client n’est **pas** la source de vérité pour `/watch/:token/vnc/vnc.html`.

Cela corrige les cas utilisateur observés :

1. **Interactif bloqué** — noVNC mémorise `view_only` dans le `localStorage` ; une session read-only précédente laissait « Afficher uniquement » coché.
2. **Read-only contournable** — un spectateur pouvait ouvrir Settings noVNC et décocher « Afficher uniquement ».

---

## Limite de sécurité (pourquoi une amélioration est nécessaire)

Le verrouillage v1 est **uniquement côté client noVNC** (HTML/JS injecté + UI masquée). Il ne contrôle pas ce qui transite sur le WebSocket VNC.

Un utilisateur déterminé peut contourner la v1 en :

- modifiant le `localStorage` ou le DOM via les outils développeur ;
- chargeant une autre page noVNC (fichiers statiques non verrouillés) pointant vers le même WebSocket ;
- envoyant des trames RFB (pointeur / clavier) avec un client VNC custom, tant qu’il possède l’URL WebSocket et le token watch valide.

Aujourd’hui, le proxy WebSocket (`apps/server/src/ws.rs`, `handle_novnc_proxy`) relaie **tous** les messages client → upstream sans inspection du protocole RFB :

```text
noVNC (navigateur)  ↔  bunny-server (proxy)  ↔  websockify  ↔  x11vnc  ↔  Chromium
```

x11vnc est démarré en mode **partagé** (`-shared`, sans `-viewonly`) pour permettre à la fois le contrôle depuis la Web UI (onglet Interactif) et la diffusion read-only sur le **même** stack navigateur par session.

**Modèle de confiance v1 :** read-only = confiance raisonnable pour un spectateur casual ; **pas** une barrière cryptographique ou protocole contre un attaquant avec accès au lien watch et compétences techniques.

---

## Objectif de l’amélioration

Garantir que le mode **read-only** ne transmet **aucun** événement pointeur/clavier au desktop, indépendamment du client noVNC ou du `localStorage`, tout en conservant le mode **interactif** pour les liens `interactive:true` et l’onglet Interactif authentifié.

---

## Pistes d’implémentation

### 1. Filtrage RFB dans le proxy WebSocket bunny-server (recommandé)

Intercepter le flux **client → upstream** dans `handle_novnc_proxy` (ou variante watch / browser avec contexte de mode).

- Parser les trames RFB binaires (types 5 PointerEvent, 4 KeyEvent, etc.).
- En contexte **read-only** : dropper les messages d’entrée, laisser passer framebuffer / encodings / keepalive.
- En contexte **interactif** : relayer sans filtre.

**Avantages :** un seul stack x11vnc par session ; enforcement indépendant du client.  
**Inconvénients :** maintenance d’un parseur RFB minimal ; tests sur encodings / versions noVNC.

Paramètres de route :

- Watch : mode depuis `watch.mode` (déjà résolu côté HTTP).
- Browser authentifié : flag explicite sur `/browser-sessions/:id/vnc/ws` (Stream = read-only, Interactif = full).

### 2. Deux instances x11vnc ou bascule `-viewonly`

- **Option A :** second port VNC read-only avec `x11vnc -viewonly` pour watch / Stream ; port interactif sans `-viewonly` pour l’UI éditeur.
- **Option B :** redémarrer ou reconfigurer x11vnc au changement de mode (plus fragile, latence).

**Avantages :** enforcement au niveau serveur VNC, pas de parseur RFB dans bunny.  
**Inconvénients :** complexité stack, ports supplémentaires, coordination lifecycle.

### 3. Restreindre l’accès aux assets noVNC non verrouillés

Servir **uniquement** `vnc.html` verrouillé sur les routes publiques watch ; refuser ou ne pas exposer les autres fichiers statiques noVNC sans auth.

Réduit le contournement « autre page noVNC », mais **ne suffit pas** seul (client VNC custom + WS).

### 4. Tokens watch à capacités séparées

JWT ou claims sur le token watch : `capabilities: ["view"]` vs `["view", "input"]`. Le proxy WS refuse les trames d’entrée si `input` absent — même approche que (1), avec modèle d’auth explicite.

---

## Critères d’acceptation (future)

1. Lien watch **sans** `interactive:true` : clic, scroll souris, clavier **n’ont aucun effet** sur Chromium, même après manipulation du DOM / localStorage / client alternatif.
2. Lien watch **`interactive:true`** : interaction complète sans régression.
3. Web UI : onglet **Stream** read-only verrouillé ; onglet **Interactif** inchangé.
4. Tests automatisés ou manuels documentés : au minimum une checklist RFB (pointer + key dropped en read-only).

---

## Fichiers concernés (implémentation future)

| Fichier | Rôle |
|---------|------|
| `apps/server/src/ws.rs` | Proxy WebSocket bidirectionnel — point d’injection filtrage RFB |
| `apps/server/src/watch.rs` | Résolution `watch.mode` → contexte read-only sur WS |
| `apps/server/src/novnc_proxy.rs` | Verrouillage HTML v1 (peut rester en défense en profondeur UI) |
| `apps/server/src/api.rs` | Route `browser_novnc_ws` — distinguer Stream vs Interactif |
| `crates/bunny-browser/src/stack.rs` | Optionnel : second VNC / `-viewonly` |
| `docs/integrations/discord.md` | Mettre à jour la doc sécurité une fois l’enforcement serveur livré |

---

## Références

- noVNC `view_only` : option **client** ; ne sécurise pas le protocole.
- RFB 3.8 : [RFC 6143](https://www.rfc-editor.org/rfc/rfc6143) — types de messages PointerEvent (5), KeyEvent (4).
- Issue / discussion interne : contournement Settings noVNC signalé en test manuel watch Discord (2025).
