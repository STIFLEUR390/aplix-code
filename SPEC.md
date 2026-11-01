# 📋 Cahier des charges fonctionnel — `aplix code` (`apc`), version CLI

**Version :** 1.1
**Statut :** spécification du MVP
**Périmètre :** le binaire en ligne de commande uniquement. L'application Tauri est anticipée par
l'architecture mais **hors périmètre**.
**Autorité :** ce document fait foi pour le MVP ; le `README.md` fait foi pour la vision produit et
le post-MVP. En cas de conflit sur un détail d'implémentation, c'est ce fichier qui l'emporte — voir
§8 pour les écarts assumés par rapport au README.

---

## 1. Vision et objectifs

**Vision :** un agent de codage en terminal **local-first, ultra-rapide et économe**, qui se
distingue par sa modularité radicale (« tout est plugin ») et son intelligence de décision locale,
tout en restant un binaire léger et facile à distribuer.

Objectifs mesurables :

- **Performance :** démarrage < 100 ms ; décisions locales (sécurité, routage) < 20 ms.
- **Empreinte :** binaire de base < 15 Mo (hors modèle). Modèle par défaut (`open-jev`) < 400 Mo.
- **Coût :** zéro coût pour les décisions locales ; coût LLM maîtrisé par le filtrage de contexte.
- **Confidentialité :** aucune donnée tierce pour les décisions locales ; les appels LLM sont la
  seule sortie réseau, vers le provider choisi. Seule exception, l'error-reporting opt-in de §3.4,
  **désactivé par défaut**.

---

## 2. Périmètre du MVP

### 2.1 Commandes CLI

| Commande | Description | Priorité |
| :--- | :--- | :--- |
| `apc run "<prompt>"` | Session d'agent non interactive (one-shot) : exécute la tâche et affiche le résultat. | **P0** |
| `apc chat` | Session interactive dans le terminal : REPL texte ligne par ligne (stdin/stdout). | **P0** |
| `apc init` | Crée un `apc.toml` commenté dans le répertoire courant. | **P0** |
| `apc model <cmd>` | Gère les modèles de décision locaux : `list`, `download`, `status`, `verify`, `remove`. | **P0** |
| `apc --version` / `-V` | Version de `apc` **et** du modèle de décision chargé. | **P0** |
| `apc config` | Affiche la configuration effective et sa provenance par couche. | **P1** |

`apc chat --continue` reprend la dernière session.

> **Pas de TUI riche en V1.** Le `chat` est un flux texte simple : pas de panneaux, pas de
> coloration complexe, pas de `ratatui`. La TUI arrive en V1.1 (voir §7).

### 2.2 Moteur d'agent

- **Boucle ReAct** : `Observation → Réflexion → Action`, avec gestion d'erreurs, timeouts et une
  limite d'itérations par tour.
- **Outils intégrés (P0)** : `read_file`, `write_file` (confirmation si le fichier existe déjà),
  `list_dir`, `run_shell` (toujours via le Permission Guard, §3.1).
- **Hooks** : événements de cycle de vie `PreToolUse`, `PostToolUse`, `Stop`. Non exposés à
  l'utilisateur en V1, mais le point d'extension doit exister dans `apc-agent`.
- **Gestion du contexte** : compaction simple (résumé des messages anciens) pour rester sous la
  fenêtre du modèle.

### 2.3 Couche de décision locale (`apc-decision`)

- **Modèle par défaut** : `open-jev` (DeBERTa-v3) quantifié `q4f16`, ~350 Mo, téléchargé à la
  demande.
- **Téléchargement reprenable** : fichier `.part`, reprise par en-tête HTTP `Range`, vérification
  SHA-256 avant activation ; un fichier corrompu n'est jamais chargé.
- **Permission Guard (P0)** : avant chaque `run_shell`, estimer la probabilité que la commande soit
  destructrice. Si `P > 0.8` → confirmation explicite de l'utilisateur. Le seuil est configurable.
- **Classification d'intention (P1)** : `generate_code`, `explain_code`, `review_code`, `other`.
- **Fallback obligatoire** : sans modèle local (pas encore téléchargé, erreur d'inférence), une
  heuristique (liste noire de motifs : `rm -rf`, `mkfs`, `dd of=/dev/`, `DROP TABLE`, …) assure le
  Permission Guard. **Le CLI ne doit jamais être bloqué par l'absence du modèle.**

L'heuristique est livrée **avant** le modèle neuronal : c'est elle qui rend le MVP utilisable dès la
première compilation, et elle reste le filet de sécurité en production.

### 2.4 Fournisseurs LLM (`apc-models`)

- **Abstraction** : un trait unique `ModelAdapter` exposant
  `chat(messages, tools) -> Stream<Response>` (streaming dès la V1, voir §8.3).
- **Fournisseurs P0** : OpenAI, Anthropic, Ollama, DeepSeek, OpenRouter, Groq — parlés en HTTP
  direct, via les deux dialectes réellement utilisés (`/chat/completions` compatible OpenAI,
  `/messages` Anthropic). Voir §8.10. Gemini attend son propre dialecte (non implémenté en v0.2).
- **Configuration** : provider et modèle dans `apc.toml`. Les clés API viennent de l'environnement
  (`ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, …), jamais du fichier de config commité.

### 2.5 Mémoire (minimale)

- **Mémoire de session** : historique de conversation en JSONL sous
  `${XDG_DATA_HOME:-~/.local/share}/aplix/sessions/`. Reprise via `apc chat --continue`.
- **Pas de mémoire long terme en V1** (schéma, oubli, distillation → V1.3).

---

## 3. Exigences non fonctionnelles

### 3.1 Sécurité (priorité absolue)

- **Permission Guard** : aucune commande shell n'est exécutée sans passer par le filtre de décision
  local. C'est le garde-fou principal du MVP.
- **Sandboxing (P1)** : en V1, exécution dans le répertoire de travail de l'utilisateur. Pas de
  Bubblewrap/Landlock/Seatbelt — mais l'architecture doit permettre de brancher un « Sandbox Plugin »
  en V2 sans toucher au reste.
- **Pas de `unsafe`** : `#![forbid(unsafe_code)]` sur tous les crates, **sans exception** en V1
  (aucun plugin natif chargé dynamiquement, donc aucune FFI à justifier).

### 3.2 Performance et distribution

- **Binaire < 15 Mo** hors modèle, via un profil de release agressif (§6).
- **Démarrage < 100 ms** : pas de runtime lourd (ni Node, ni Python), pas de scan de plugin au
  démarrage.
- **Distribution « lite »** : aucun modèle embarqué ; téléchargement au premier lancement ou via
  `apc model download`.

### 3.3 Maintenabilité et extensibilité

- **Workspace Cargo** découpé par responsabilité (§4).
- **« Tout est plugin »** : chaque couche (modèle, outils, mémoire, décision) définie par un trait
  dans `apc-traits`.
- **Qualité** : `cargo clippy --all-targets -- -D warnings` et `cargo fmt --check` passent ; tests
  unitaires + intégration par crate.

### 3.4 Télémétrie et diagnostic (opt-in, désactivée par défaut)

- **But** : diagnostiquer erreurs et panics en production sans trahir la promesse « aucune
  télémétrie » du README. Ce n'est pas de la télémétrie produit : aucune métrique d'usage, jamais de
  contenu utilisateur.
- **Protocole** : [Sentry](https://crates.io/crates/sentry) (`sentry` + `tracing` /
  `tracing-subscriber`), donc compatible avec tout DSN Sentry ou GlitchTip auto-hébergé.
- **Défaut : désactivée.** Sans DSN, le client est entièrement inactif : `ClientOptions::new()` laisse
  `dsn = None` et `sentry::Client::with_options` renvoie un client désactivé (`is_enabled() == false`).
- **Activation** : un DSN dans l'environnement (`APC_SENTRY_DSN`, sinon `SENTRY_DSN`) **et**
  `telemetry = true` dans `apc.toml`. Le DSN n'est jamais écrit dans un fichier commité.
- **Désactivation** : `APC_TELEMETRY=0` l'emporte sur tout (même un DSN présent) ; sinon absence de
  DSN ; sinon feature `telemetry` non compilée (`--no-default-features`).
- **Contenu envoyé** : erreurs, panics, et breadcrumbs `tracing` de niveau `warn`/`error` uniquement
  par défaut. Jamais le contenu des fichiers, des prompts ni des réponses du modèle. Le suivi de
  session (`release-health`) est désactivé : ce sont des métriques d'usage.
- **Compilation opt-in** : depuis la v1.0, `default = []` — un `cargo build` normal ne contient
  aucun code Sentry. Pour travailler dessus : `--features telemetry`, `telemetry = true` et un DSN.
  Le `.env` local (non commité) ne sert plus qu'à ça. Vérifié : ni crate `sentry` dans le graphe, ni
  DSN dans le binaire, et aucune connexion même avec `SENTRY_DSN` + `APC_TELEMETRY=1` forcés.
- **Limite connue** : `panic = "abort"` (profil release) supprime le déroulement de pile, donc le
  hook de panic Sentry ne peut rien envoyer en release ; seules les erreurs capturées explicitement
  partent. En debug (unwind) les panics sont remontées.
- **Câblage** : le subscriber `tracing` (logs locaux, toujours actifs) et le layer Sentry sont montés
  dans la racine de composition `apc-cli`. Aucune crate de bibliothèque n'initialise de client.
- **Verbosite locale** : `APC_LOG=debug|info|warn|error` (défaut `warn`) règle le niveau du
  subscriber `tracing`, sans réseau ni DSN.

---

## 4. Architecture cible

```
crates/
  apc-core/      types partagés, erreurs, config      — aucune dépendance interne
  apc-traits/    tous les traits                      — le contrat de plugin
  apc-agent/     boucle d'agent, orchestration        — aucune I/O directe
  apc-tools/     outils intégrés (fs, shell)
  apc-decision/  Permission Guard : heuristique puis open-jev
  apc-models/    adapters LLM (genai)
  apc-memory/    session JSONL (+ backends futurs)
  apc-plugins/   registre statique des plugins
  apc-cli/       le binaire `apc` — racine de composition
skills/          définitions de skills en Markdown (V1.1+)
```

Direction des dépendances : `apc-core` en bas, `apc-traits` au-dessus, les crates de plugins
dépendent des deux, `apc-agent` ne connaît que les traits, `apc-cli` assemble les implémentations
concrètes. **Aucune dépendance spécifique à un provider ou à un backend dans `apc-agent` ni
`apc-cli`.**

En V1, `apc-plugins` n'est qu'un **registre statique** (nom → `Box<dyn Tool>` / `Box<dyn
DecisionProvider>`) : il évite de câbler les outils en dur dans `apc-cli` et prépare le chargement
dynamique de la V1.2 sans changer les appelants.

Traits principaux :

```rust
#[async_trait]
pub trait ModelAdapter: Send + Sync {
    async fn chat(&self, messages: &[Message], tools: &[ToolSpec]) -> Result<ChatStream>;
}

#[async_trait]
pub trait DecisionProvider: Send + Sync {
    async fn noul(&self, state: &str, question: &str) -> Result<f64>;
    async fn choice(&self, state: &str, question: &str, options: &[&str]) -> Result<(String, f64)>;
}

pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn schema(&self) -> serde_json::Value;
    async fn execute(&self, args: serde_json::Value) -> Result<String>;
}

/// Confirmation humaine avant une action gardée (implémentée par le CLI, jamais par un outil).
pub trait Approver: Send + Sync {
    fn confirm(&self, command: &str, risk: f64) -> Result<bool>;
}

pub trait MemoryStore: Send + Sync {
    async fn append(&self, session: &SessionId, message: &Message) -> Result<()>;
    async fn load(&self, session: Option<&SessionId>) -> Result<Vec<Message>>;
}

pub trait Plugin: Send + Sync {
    fn name(&self) -> &'static str;
    fn init(&self, ctx: &mut PluginContext) -> Result<()>;
}
```

---

## 5. Matrice concurrentielle et inspirations

| Concurrent | Philosophie / point fort | À reprendre | À éviter |
| :--- | :--- | :--- | :--- |
| **Codex CLI** | Sécurité niveau entreprise, sandboxing multi-plateforme, workspace de 60+ crates. | Structure modulaire du workspace ; séparation entre **politique d'exécution** (`execpolicy`) et sandboxing. | La complexité écrasante ; l'ancrage fort dans l'écosystème OpenAI. |
| **`pi_agent_rust`** | Binaire unique, démarrage instantané, extensions « capability-gated », zéro `unsafe`. | Le modèle de sécurité des extensions ; l'objectif de démarrage instantané ; le zéro `unsafe`. | Le portage fidèle d'un agent existant, qui laisse peu de place à une philosophie de plugins radicale. |
| **`harness-rs`** | L'architecture la plus proche de la vision d'`apc` : Skills, Memory, Hooks, Sensors, compaction progressive, proc-macro `#[skill]`. | La référence architecturale majeure : séparation en couches, concept de **Sensors** (`cargo check` comme signal), compaction progressive. | La maturité (0.0.x) et la tentation de tout implémenter dès le MVP. |
| **`crab-code`** | Clone Rust de Claude Code (~161k LOC), permissions multi-modes, config en couches. | Les **modes de permission** (default, acceptEdits, dontAsk, bypassPermissions, plan, auto) et la résolution de config en couches. | La taille et la complexité ; `apc` doit avoir sa propre identité. |
| **`opencode-rust`** | Architecture serveur/client : serveur HTTP local, CLI/TUI client. | Le modèle serveur local, qui prépare l'intégration Tauri. | Le pont de plugins en Node.js, incompatible avec un binaire Rust unique. |
| **`local-jev`** | Modèle de décision local, offline, <18 ms, API simple. | Le contrat HTTP `/v1/systemone/choice` comme backend de décision (sidecar), avec fallback heuristique obligatoire. Voir §8.11. | La dépendance à un mainteneur unique → **fallback heuristique obligatoire**. |

---

## 6. Stack technique

| Composant | Crate(s) | Justification |
| :--- | :--- | :--- |
| Parsing CLI | `clap` (derive) | Standard de facto, mature. |
| Multi-providers LLM | `reqwest` + `futures` (adaptateurs propres) | Voir §8.10 : deux dialectes suffisent, `genai` écarté. |
| Boucle d'agent | implémentation propre dans `apc-agent` | Voir §8.2 : `rig-core` écarté. |
| Décision locale | `local-jev` (derrière un feature flag) + heuristique | Voir §8.1. |
| Téléchargement modèle | `reqwest` + `sha2` + `futures` | Voir §8.4 : reprise `Range` et digest faits à la main. |
| Configuration | `toml` + `serde` | Standard, lisible. |
| Mémoire de session | `serde_json` + `std::fs` (JSONL) | Simple, robuste, débuggable. Pas de base pour le MVP. |
| Chemins | `directories` | Respect de `XDG_DATA_HOME` / `%APPDATA%` / `~/Library`. |
| Erreurs | `thiserror` (libs) + `anyhow` (CLI) | Séparation erreurs de bibliothèque / applicatives. |
| Async | `tokio` | Runtime de `reqwest` et des commandes. |
| Télémétrie (opt-in) | `sentry` + `tracing` + `tracing-subscriber` (feature `telemetry`, off par défaut) | Voir §3.4 et §8.9 ; `sentry` n'est compilé que si la feature est active, pour ne pas peser sur le budget de 15 Mo. |
| TUI (V1.1) | `ratatui` + `crossterm` | Hors MVP. |

Profil de release :

```toml
[profile.release]
lto = true
codegen-units = 1
strip = "symbols"
panic = "abort"
opt-level = 3      # voir §8.5 : la vitesse primait sur la taille
```

---

## 7. Feuille de route

| Version | Contenu |
| :--- | :--- |
| v0.1 | Squelette du workspace, `apc init`, résolution de config, `apc config` |
| v0.2 | `apc run` + `apc chat` (REPL texte), streaming SSE, adaptateurs HTTP directs |
| v0.3 | Boucle d'agent + outils intégrés ; Permission Guard heuristique sur `run_shell` |
| v0.3b | Backend `open-jev` derrière un feature flag : client du sidecar `local-jev` + `Cascade` heuristique |
| v0.4 | `apc model` + téléchargement reprenable |
| v0.5 | Télémétrie opt-in : logs `tracing` locaux + Sentry désactivé par défaut |
| v1.0 | MVP stable : CLI, docs, CI, releases Linux/macOS/Windows |
| v1.1 | TUI `ratatui`, classification d'intention, `skills/` |
| v1.2 | Plugins de recherche web (SearXNG) et d'extraction (Obscura) |
| v1.2b | Décider si `rig` remplace nos adapters et notre boucle d'agent (voir §8.2) |
| v1.3 | Mémoire long terme (SQLite + FTS5 ou `sqlite-vec`) |
| v2.0 | `apc serve` (serveur HTTP local) + application Tauri |
| v2.1 | Sandboxing avancé (`landlock`/`seatbelt`) ou plugins WASM |

---

## 8. Décisions et écarts assumés par rapport au README

Chaque écart est volontaire et motivé. Le README reste la référence pour la vision et le post-MVP.

### 8.1 `local-jev` est un feature flag, pas une dépendance obligatoire

Le modèle neuronal ne peut pas être dans le chemin critique : il pèse ~350 Mo, il est téléchargé à
la demande, et son runtime d'inférence est le poste de risque n°1 pour le budget de 15 Mo. Donc :

- V1 livrable = heuristique seule, `apc` fonctionne dès `cargo build`.
- `apc-decision` expose `DecisionProvider` ; `JevProvider` est derrière le feature `open-jev`,
  éteint par défaut, et interrogé via `decision.endpoint` (ou `APC_DECISION_ENDPOINT`).
- L'heuristique reste le fallback permanent, jamais du code mort : `Cascade` bascule sur elle à la
  moindre erreur de transport, donc un sidecar absent ou bloqué ne bloque pas le CLI.

Cela répond directement à l'exigence « le CLI ne doit jamais être bloqué par l'absence du modèle ».

### 8.2 `rig-core` est écarté

Le cahier des charges initial proposait `rig-core` pour les abstractions **et** `genai` pour les
providers. Les deux sont des frameworks LLM complets, chacun avec ses propres `Message`, `Tool` et
formats de réponse : les combiner impose une couche de conversion permanente, deux jeux de types à
maintenir, et surtout **enferme `apc-traits` dans les types de `rig`** — l'inverse de « tout est
plugin ». `apc-core` définit ses propres types et `apc-models` les traduit en HTTP. Une centaine de
lignes de boucle ReAct écrites à la main coûtent moins cher que cette dépendance.

**Réévaluation planifiée (v1.2b).** La décision ci-dessus reste valable pour le MVP, mais `rig` a
beaucoup bougé depuis (API 0.44+) et couvre désormais ce qui nous manque encore : `Tool` typé avec
`ToolAnnotations` (de quoi porter le Permission Guard au niveau de l'outil), `AgentRunner` avec
`max_turns` et `stream()`, `AgentHook` (`on_run_start`, `on_completion_call`, `on_model_select`,
deltas `TextDelta`/`ReasoningDelta`/`ToolCallDelta`) pour les hooks et la compaction, et des stores
vectoriels pour la mémoire longue. La tâche kaneo `d6k5r9tql0itnp3c82l5vzfs` tranche : adopter,
adopter partiellement (un seul crate), ou refuser — avec la taille du binaire et `cargo tree` comme
mesures, et la contrainte inchangée qu'aucun type de `rig` ne remonte au-dessus de `apc-models` /
`apc-agent`. Docs : <https://rig.rs/docs>.

### 8.3 Le streaming est dans le trait dès la V1

`chat` renvoie un flux, pas une réponse complète. Rétrofitter le streaming plus tard obligerait à
modifier le trait, l'agent et le CLI en même temps ; et un agent muet pendant 30 s est une mauvaise
expérience. `apc run` affiche le flux au fur et à mesure ; `apc chat` fait de même, tour après tour.

### 8.4 Pas de `http-ferry` : `reqwest` + `sha2`

`http-ferry` est peu connu et non vérifié ; `reqwest` gère `Range` et sert déjà aux appels LLM
(§8.10). La reprise + SHA-256 représente ~100 lignes testables, contre une dépendance
supplémentaire au budget de taille et à la surface d'audit.

### 8.5 `opt-level = 3`, pas `"z"`

Le cahier des charges initial visait `opt-level = "z"` pour tenir sous 15 Mo. Or les objectifs
mesurables sont des objectifs de **vitesse** (< 100 ms au démarrage, < 20 ms par décision), et
`"z"` dégrade le code au profit de la taille. Un binaire de cette ampleur tient largement sous
15 Mo avec `strip = "symbols"` + `lto` + `panic = "abort"`. On garde `opt-level = 3` et on mesure
(`ls -lh target/release/apc`) ; si le budget est dépassé, on redescend à `"s"` avant `"z"`.

### 8.6 `apc chat` est un REPL texte en V1

Le tableau des commandes disait « TUI » et la section des limites disait « pas de TUI riche » : les
deux ne peuvent pas être vrais. Le README est cohérent (TUI riche = v1.1, `ratatui`). V1 = flux
texte simple ; `ratatui` + `crossterm` arrivent en V1.1.

**Livré en v1.1** : `crates/apc-cli/src/tui.rs` — transcript scrollable, saisie une ligne, spinner,
flux en direct, et le Permission Guard affiché dans la barre de statut (`y`/`n`) au lieu d'écraser
la vue. `apc chat` ne l'utilise que si stdin **et** stdout sont des terminaux ; sinon il retombe sur
le REPL texte, ce dont dépendent les tests et les scripts. La classification d'intention est un classifieur
heuristique local (`apc_decision::classify` : `generate_code`, `explain_code`, `review_code`,
`other`) qui choisit le skill à charger : si le projet a `skills/<nom>.md`, son contenu est inséré en
tête de conversation comme message système, sans être persisté. Pas de fichier, pas de prompt
système ; aucun modèle local n'est nécessaire.

### 8.7 Neuf crates conservés (dont `apc-memory` et `apc-plugins`)

Le cahier des charges initial listait sept crates, sans `apc-memory` ni `apc-plugins`, alors que
§2.5 impose la persistance de session (P0) et que §5.4 demande les traits `Plugin`/`MemoryStore`.
Un crate `memory` vide aurait été du poids mort, mais la mémoire de session est du code réel dès le
MVP ; et `apc-plugins` justifie sa place comme registre statique (éviter le câblage en dur dans le
CLI). Le découpage en neuf crates du README est donc conservé — c'est aussi ce qui rend crédible la
promesse « tout est plugin ».

### 8.8 Aucune exception à `forbid(unsafe_code)`

Le cahier des charges prévoyait une exception pour le chargement de plugins natifs. Comme le
chargement dynamique est hors périmètre V1, l'exception disparaît : `#![forbid(unsafe_code)]` sur
**tous** les crates, sans dérogation. Elle reviendra avec la V1.2, dans un crate dédié et isolé.

### 8.9 La télémétrie est opt-in, pas interdite

Le README promet « no telemetry ». Une intégration Sentry n'est pas de la télémétrie produit : elle
ne collecte aucune métrique d'usage et ne s'active que sur décision explicite de l'utilisateur, avec
son propre DSN. Les deux coexistent à trois conditions, toutes vérifiables dans le code :
**désactivée par défaut**, **aucun DSN livré**, **jamais de contenu utilisateur**. C'est la seconde
sortie réseau possible après le provider LLM, et elle est nulle tant que l'utilisateur ne l'a pas
armée. Le README et `AGENTS.md` sont alignés sur cette exception.

### 8.10 `genai` est écarté au profit d'adaptateurs HTTP directs

Le cahier des charges prévoyait `genai` pour couvrir sept fournisseurs. Deux raisons l'ont fait
tomber au moment d'implémenter la v0.2 :

1. **Le besoin réel est plus étroit.** Six des sept fournisseurs cibles parlent le dialecte OpenAI
   `/chat/completions` (OpenAI, DeepSeek, OpenRouter, Groq, Ollama, plus toute passerelle locale
   compatible) ; Anthropic a son propre `/messages`. Deux corps de requête et deux lecteurs SSE
   couvrent donc tout le P0, en ~250 lignes testables hors ligne.
2. **Le coût d'entrée mesurable.** `genai` n'était pas dans le cache local et crates.io répondait à
   ~4 Ko/s sur la machine de développement : l'arbre de dépendances (SDK cloud, résolveurs d'auth,
   son propre TLS) n'était pas installable dans un délai raisonnable. Ce n'est pas un argument
   d'architecture, mais c'est une donnée : la dépendance pèse plus que le code qu'elle remplace.

Conséquences assumées :

- `apc-models` possède son `reqwest::Client` et son `ModelAdapter` (`apc-traits`), avec streaming
  SSE dès la v0.2.
- **Gemini n'est pas implémenté** : `provider.name = "gemini"` renvoie une erreur explicite. Son
  protocole (`generateContent`, streaming par `?alt=sse`) est un troisième dialecte, à ajouter quand
  le besoin apparaît.
- Les clés API se lisent dans `APC_API_KEY` puis dans la variable du fournisseur
  (`OPENAI_API_KEY`, …), jamais dans `apc.toml`. `provider.base_url` (ou `APC_BASE_URL`) permet de
  pointer une passerelle locale.
- Si un fournisseur exotique (Bedrock, Vertex, Cohere) devient nécessaire, réévaluer `genai` à ce
  moment-là plutôt que de réimplémenter son SDK.

### 8.11 `open-jev` interroge un sidecar, il n'embarque pas de runtime d'inférence

La v0.3b devait brancher le modèle `open-jev` derrière un feature flag. Trois contraintes ont
orienté la forme prise :

1. **Aucun runtime d'inférence n'est disponible ici.** `candle`, `ort`, `tract`, `tokenizers` ne sont
   pas dans le cache local, le réseau plafonne à ~4 Ko/s et aucune pondération n'est sur le disque.
   Embarquer le modèle aurait rendu le feature infalsifiable sans réseau.
2. **`local-jev` est déjà l'articulation prévue.** Il expose exactement l'API de décision visée
   (`/v1/systemone/choice`, `/noul`, `/score`) et sait tourner sans pondérations (moteur lexical
   `ZeroWeightFast`), donc un utilisateur peut le lancer à côté d'`apc` sans rien télécharger.
3. **Le sous-processus reste optionnel.** Le contrat est du HTTP local : pas de FFI, pas de
   chargement dynamique, donc `#![forbid(unsafe_code)]` tient (SPEC §8.8) et `apc` continue de
   fonctionner seul.

Conséquences assumées :

- `JevProvider` (`apc-decision::jev`, feature `open-jev`) est un client ; il lit la probabilité de
  l'option `yes` dans `probabilities`, **pas** `confidence` (qui décrit seulement le gagnant).
- La route `noul` de local-jev classe au lieu de répondre oui/non : `DecisionProvider::noul` est donc
  émis comme un `choice(["no", "yes"])`.
- `Cascade` fait du fail-open : toute erreur (connexion, statut HTTP, JSON) rend la main à
  l'heuristique, sans message d'erreur pour l'utilisateur.
- **Une réponse plate vaut absence d'avis.** Mesuré sur `local-jev` v2.0.0 installé tel quel : sans
  pondérations, son moteur lexical répond `0.5 / 0.5` à toute question. Lue comme un risque faible,
  cette réponse laissait passer `rm -rf` sans confirmation. `JevProvider` traite donc un écart
  < 0.01 entre les deux meilleures options comme une erreur, et l'heuristique reprend la main. Le
  jour où un backend à poids réels arrive, cette garde ne change rien à ses réponses informatives.
- `decision.endpoint` est le seul réglage ajouté ; sans lui, le feature ne change rien au binaire.
- Conséquence ouverte : le modèle ne donne pas encore la qualité « DeBERTa q4f16 » promise par le
  README ; ce sera vrai quand un backend d'inférence embarqué sera réellement mesurable.

### 8.12 `apc model` existe, le catalogue est vide

La v0.4 livre tout le chemin de téléchargement (`.part`, reprise par `Range`, SHA-256, renommage
atomique, `status`, `verify`, `remove`) mais **aucune entrée de catalogue**. Motif : `local-jev`
v2.0.0 ne publie aucune pondération et n'embarque aucun moteur d'inférence (§8.11), donc il n'existe
pas d'artefact à télécharger. Inventer une URL et une empreinte aurait rendu `apc model download`
mensonger.

Conséquences :

- `apc model list` affiche que rien n'est publié ; `apc model download <id>` échoue avec
  `unknown model`, sauf après ajout d'une entrée dans `CATALOG`
  (`crates/apc-decision/src/store.rs`).
- Le moteur est testé de bout en bout contre un serveur local : reprise, serveur qui ignore `Range`,
  somme invalide jamais installée, vérification d'un fichier altéré, suppression.
- Le jour où un artefact réel apparaît, l'ajout est une entrée de quatre champs ; rien d'autre à
  changer.

### 8.13 `fetch_page` lit le web en HTTP, sans Obscura

La feuille de route v1.2 prévoyait un `fetch_page` adossé à Obscura, « navigateur headless exposé en
HTTP ». Vérification faite sur la version installée (0.2.5) : `obscura serve` est un serveur **CDP**
(`/json/version`, `/json/list`, contenu uniquement par WebSocket) et `obscura fetch` est une commande
shell ; il n'existe aucun endpoint HTTP de récupération. Trois voies ont été pesées :

- **client CDP maison** (~200 lignes de WebSocket, `Sec-WebSocket-Accept` compris) : aucun crate
  WebSocket n'est en cache local, donc `cargo build --offline` ne peut pas l'ajouter, et c'est du
  protocole à maintenir pour un seul outil ;
- **la crate `obscura`** comme bibliothèque : dépendance git qui compile V8 depuis les sources —
  build hors-ligne impossible, chaîne C++ exigée, et la taille exploserait le plafond de 15 Mo ;
- **`obscura mcp --http`** : contrat réel et `reqwest` suffirait, mais impose de lancer un second
  service et de parler MCP/JSON-RPC depuis l'outil.

La v1.2 livre donc `fetch_page` en `GET` direct (`reqwest`) suivi de **`rs-trafilatura` 0.2**, le port
Rust de trafilatura : classification du type de page, extraction du contenu principal (navigation,
pieds de page et publicités retirés) et sortie Markdown — les blocs de code des pages de
documentation survivent, contrairement à un simple aplatissement de balises. Le texte est ensuite
tronqué à `tools.fetch_max_bytes` (64 Kio par défaut) sur une frontière de caractère, avec le nombre
d'octets perdus. Les pages rendues en JavaScript ne sont pas couvertes.

Coût mesuré de la dépendance : crate de 229 Ko, graphe déjà présent dans le cache local
(`dom_query`, `html5ever`), build release `--no-default-features` de 3 939 584 → 8 147 656 octets,
soit toujours sous le plafond de 15 Mo. En build **debug** la crate écrit des lignes `DEBUG:` sur
stderr (`cfg!(debug_assertions)`) : le binaire publié, lui, est silencieux.

Conséquences :

- `tools.obscura_url` n'existe pas : il n'y a pas de service à configurer, donc `fetch_page` est
  toujours enregistré. `tools.searxng_url` (ou `APC_SEARXNG_URL`) est la seule URL, et sans elle
  `web_search` n'apparaît pas dans le schéma envoyé au modèle — aucun appel sortant, aucune erreur
  au démarrage.
- `obscura mcp --http` reste la voie de reprise si le rendu JavaScript devient nécessaire : l'ajout
  est un backend derrière le `Tool` déjà en place.

---

## 9. Hors périmètre V1 (explicite)

1. Application Tauri (l'architecture la prépare via `apc serve`, aucun code écrit).
2. Recherche web (SearXNG) et extraction (Obscura, Firecrawl, Crawl4AI).
3. Mémoire long terme / knowledge graph : seule la session JSONL existe.
4. Chargement dynamique de plugins (`.so`/`.dylib`/WASM) : les traits existent, les plugins V1 sont
   compilés en dur.
5. Multi-agents : un seul agent.
6. Sandboxing avancé (Bubblewrap, Landlock, Seatbelt).
7. TUI riche (`ratatui`).
8. Collecte de métriques d'usage / analytics : seule l'error-reporting opt-in de §3.4 existe.
