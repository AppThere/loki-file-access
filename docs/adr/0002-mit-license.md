# ADR 0002: MIT License

## Status

Accepted

## Context

`loki-file-access` is a general-purpose utility crate for cross-platform file
access.  It is intended for broad adoption across the Rust ecosystem, including
use in projects under a variety of open-source and proprietary licenses.

The two most common choices for Rust crates are MIT and Apache-2.0 (often
dual-licensed as `MIT OR Apache-2.0`).  A license decision is needed.

## Decision

Use the **MIT license** exclusively (not dual-licensed with Apache-2.0).

## Rationale

- **Maximum compatibility**: MIT is compatible with a strictly wider range of
  downstream projects than Apache-2.0.  Some projects and organisations cannot
  accept Apache-2.0 due to concerns about the patent clause (Section 3) — for
  example, projects under GPLv2-only or certain corporate policies.  MIT has
  no patent clause, removing this friction.

- **No strategic IP**: As a low-level utility crate with no novel algorithms or
  patentable inventions, the patent protection offered by Apache-2.0 provides
  negligible benefit to the project or its contributors.

- **Simplicity**: A single license is easier to understand, audit, and comply
  with than a dual-license arrangement.  Downstream consumers do not need to
  choose between two options or evaluate which one to apply.

- **Ecosystem norms**: Many widely-used Rust crates (serde, rand, base64, etc.)
  are MIT-licensed.  Using the same license reduces cognitive overhead for
  consumers evaluating dependency licenses.

## Consequences

### Positive

- Downstream projects under **any** OSI-approved license can use this crate
  without restriction.
- Contributors do not need to agree to a CLA or patent grant beyond what MIT
  already provides.
- License compliance is straightforward: include the copyright notice and
  permission notice.

### Negative

- No explicit patent grant from contributors.  If a contributor holds a patent
  that reads on the crate's functionality, MIT alone does not provide an
  express license to those patent claims.  This is an acceptable risk given the
  crate's nature as a utility library.
- Cannot be relicensed to Apache-2.0 later without consent from all copyright
  holders (though this is unlikely to be needed).
