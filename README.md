# Hexbee
This is an experiment to make a simple AI more intelligent by managing its memory, and carefully using it as an organ in reasoning in a larger runtime. 
Local-first agent runtime in Rust. The **kernel** owns memory, legality, persistence, and permissions. Language models are narrow **organs** behind typed contracts. V1 ships a playable **tavern RPG hex** with file-backed campaigns. 

## Quick start

```bash
# optional local model for dialogue / competency scoring
ollama pull qwen2.5:0.5b

cargo run -p hexbee-cli
```

At the prompt:

```text
> Play Tavern
> Play Cards
```

Environment:

| Variable | Meaning |
|----------|---------|
| `HEXBEE_MODEL` | Ollama model tag (default `qwen2.5:0.5b`) |
| `HEXBEE_SEED` | RNG seed for deterministic combat/checks |
| `HEXBEE_LOG_FILE` | Log path (default `logs/hexbee.log`) |
| `HEXBEE_LOG_STDERR` | Set `1` to also mirror logs to stderr |
| `RUST_LOG` | tracing filter |

## Architecture

```
hexbee-cli (shell)
  └── hexbee-kernel (interpreter, capability gate, commands)
        ├── hexbee-store (campaign files — source of truth)
        ├── hexbee-provider (LLM client)
        └── hexbee-hex-rpg (tavern module + organs)
        └── hexbee-hex-cards (21, Hearts, five-card draw)
```

## Campaign layout

Bundled campaign lives at `crates/hexbee-hex-rpg/campaign/`:

```
campaign/
  hex.toml          # module id, grants
  world.yaml        # setting, hard flags, soft details
  goals.yaml        # GM completion criteria
  scenes/*.yaml     # locations
  actors/*.yaml     # character sheets
  secrets.yaml      # authored checks (RPG)
  state.json        # current scene, actor, runtime
  transcript.jsonl  # committed beats
  snapshots/        # last commit for undo stub
```

Progress auto-saves each turn. Kill the process and reload with `load <path>` or restart `Play Tavern` from the bundled copy.

## Kernel commands (work mid-scene)

| Command | Effect |
|---------|--------|
| `help` / `?` | Deterministic commands + free-form play prompt |
| `options` | Provider, model, actor, continue budget |
| `save` | Write campaign folder |
| `load <path>` | Open a campaign folder |
| `actor <id>` / `next` | Hot-seat actor switch |
| `goal` | GM goal status from store |
| `grants` / `locks` | Capability table / pending locks |
| `metrics` | In-process counters |
| `reset` / `restart` / `new game` | Wipe progress and start fresh (scene, HP, transcript) |
| `quit` | Save-if-dirty and exit |

## Gameplay

- Open-ended chat → interpreter → fixed action catalog → Rust rules (dice, HP, gold, endings)
- Models paint prose; Rust decides outcomes
- Hot-seat: `actor sally` or `next` to speak as another controllable sheet

See `docs/hexbee_design.md` for the full design document.

## Workspace

| Crate | Role |
|-------|------|
| `hexbee-core` | Config, logging, metrics |
| `hexbee-store` | Campaign load/save, patches, transcript |
| `hexbee-kernel` | Interpreter, capability gate, kernel loop |
| `hexbee-provider` | LLM trait + Ollama client |
| `hexbee-hex-rpg` | Tavern hex module + organs |
| `hexbee-hex-cards` | Cards hex (21, Hearts, five-card draw) |
| `hexbee-cli` | REPL binary `hexbee` |

## Tests

```bash
cargo test --workspace
```

Includes persistence round-trip, capability gate deny, and hot-seat transcript tests.
