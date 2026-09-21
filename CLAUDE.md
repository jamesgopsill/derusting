# CLAUDE.md — derusting review workflow

This is a **review and proposal** environment for the derusting codebase
(Decentralised Trusted Manufacturing in Rust). Building, running, and
testing happen elsewhere — do not attempt them here.

## Do not edit files directly

**You are only permitted to add comments to the files.** This applies to every
file under `src` and `libderusting`. **No other files should be edited** without exception.

Please show me the comments you add and they were added.

Read-only actions are fine without asking: viewing files, searching
(grep/ripgrep/fd), running `git diff`/`git log`/`git blame`, and
generating the patches described above.

## Subdirectory CLAUDE.md files

This codebase mixes Rust (the derusting staticlib) with vendored/adjacent
C code (Prusa Buddy firmware, LWIP, FreeRTOS integration). Subdirectories
may have their own `CLAUDE.md` with context specific to that area (e.g.
module purpose, conventions, gotchas).

- Check for a `CLAUDE.md` in the current directory and any parent
  directory whenever you move into a new part of the tree — don't rely
  only on having read this root file once at the start of the session.
- Subdirectory files **add** context; they do not relax the no-direct-edit
  rule above unless they say so explicitly and specifically.
- If you're referencing or acting on a subdirectory's contents and its
  CLAUDE.md hasn't been read yet in this session, read it before
  proposing changes there.

## Project shape (context for review)

- Rust staticlib, compiled into the Prusa Buddy Firmware for Prusa Mini
  printers.
- Interacts with the LWIP network stack.
- Runs as an Embassy async task inside a FreeRTOS static task.
- Uses `std::fs` bindings for reading/writing a USB stick.
