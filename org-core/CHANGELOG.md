# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed

- Skip non-UTF-8 agenda files for reads (`WorkspaceIndex::skipped_files`) and
  refuse writes to them with `non_utf8`, instead of failing every command.
- Treat an `:ID:` that also appears in a non-agenda org file under the roots as
  `duplicate_id`, for ID refs and for hash refs to that heading.
- Apply `agents.capture_to = "inbox"` only to MCP captures, route them to the
  configured `agents.inbox_template`, and drop their planning; validate both
  settings at load time.
- Insert capture titles, bodies and prompt answers literally instead of
  expanding `%U`, `%i` or `%t` typed inside them.

### Added

- Support the `%u`, `%T` and `%%` capture template escapes.

## [0.0.2](https://github.com/johnnybravo-xyz/agd/compare/agd-core-v0.0.1...agd-core-v0.0.2)

### ✨ Features


- *(agenda)* Show past-scheduled TODO tasks in today's agenda view ([#208](https://github.com/johnnybravo-xyz/agd/pull/208)) - ([665744b](https://github.com/johnnybravo-xyz/agd/commit/665744b39c9b5f7a6a4230af8e8da384aa584a2b))
- *(mcp)* Add tags parameter to list files tool ([#79](https://github.com/johnnybravo-xyz/agd/pull/79)) - ([1b02b24](https://github.com/johnnybravo-xyz/agd/commit/1b02b243cd9d3f478cd46b682183e098b408c93f))
- *(org)* Add org-update-todo tool and CLI command for updating TODOs ([#213](https://github.com/johnnybravo-xyz/agd/pull/213)) - ([8316984](https://github.com/johnnybravo-xyz/agd/commit/831698408cfd4faf30046a45e3c452843c97660f))
- *(org)* Implement capture functionality ([#171](https://github.com/johnnybravo-xyz/agd/pull/171)) - ([a3690d4](https://github.com/johnnybravo-xyz/agd/commit/a3690d4c0b1654a49d12f2d1a0318aaee0174c8a))
- *(test)* Improve code coverage ([#20](https://github.com/johnnybravo-xyz/agd/pull/20)) - ([c8cb368](https://github.com/johnnybravo-xyz/agd/commit/c8cb368d64a0a4483171b463d77d8e8b584a9bcb))
- Add lossless org checks and clean integrations - ([fa0f0db](https://github.com/johnnybravo-xyz/agd/commit/fa0f0db843c59c085ec4471782aee557320d8259))
- Publish CLI as agd - ([41b4841](https://github.com/johnnybravo-xyz/agd/commit/41b484160d82974e7b8b29d2959845ef5c72c45a))
- Extend org-update-todo with title, body, and property drawer support ([#216](https://github.com/johnnybravo-xyz/agd/pull/216)) - ([89524c0](https://github.com/johnnybravo-xyz/agd/commit/89524c065666bd39b25ff8d2e5dda346442fc741))
- Add agent skills plugin for Claude Code, Codex, Cursor, and OpenCode ([#214](https://github.com/johnnybravo-xyz/agd/pull/214)) - ([ca88d64](https://github.com/johnnybravo-xyz/agd/commit/ca88d6418ba9958ebe75423968f4a4d4b815dbbb))
- Implement agenda view functionality ([#86](https://github.com/johnnybravo-xyz/agd/pull/86)) - ([87e412d](https://github.com/johnnybravo-xyz/agd/commit/87e412dbcf5136972afb74ee0dd97400e3f883a3))
- Add tag filtering support for search and list operations ([#71](https://github.com/johnnybravo-xyz/agd/pull/71)) - ([ea9c9f1](https://github.com/johnnybravo-xyz/agd/commit/ea9c9f163cdb7be7d82c0c475edce7e87211aa94))
- Add TOML-based configuration system ([#45](https://github.com/johnnybravo-xyz/agd/pull/45)) - ([2bfbdad](https://github.com/johnnybravo-xyz/agd/commit/2bfbdad28f3aa4ad40c518f696bdddd0bc73cc69))
- Implement search tool ([#30](https://github.com/johnnybravo-xyz/agd/pull/30)) - ([a3bac6a](https://github.com/johnnybravo-xyz/agd/commit/a3bac6a55f1ffdfd51ffadb20486b2fac21c4223))

### 🐛 Bug Fixes


- *(deps)* Update rust crate rmcp to 0.8.0 ([#42](https://github.com/johnnybravo-xyz/agd/pull/42)) - ([dc6721b](https://github.com/johnnybravo-xyz/agd/commit/dc6721b4dbb2a0b494d24d89c236a2791fb723b9))
- *(deps)* Update rust crate rowan to 0.16 ([#18](https://github.com/johnnybravo-xyz/agd/pull/18)) - ([42991e5](https://github.com/johnnybravo-xyz/agd/commit/42991e584271d46961ff2cfb36fe0b3bda7aac19))
- Correct method call order in last_day_of_month ([#129](https://github.com/johnnybravo-xyz/agd/pull/129)) - ([8fabc3e](https://github.com/johnnybravo-xyz/agd/commit/8fabc3e889dc6e2021589beadeb7479d6e5fd7cc))
- Update integration test ([#117](https://github.com/johnnybravo-xyz/agd/pull/117)) - ([b282630](https://github.com/johnnybravo-xyz/agd/commit/b2826308cff22c5ac424f2a6ea769f885d18718e))
- Update integration test flakiness ([#108](https://github.com/johnnybravo-xyz/agd/pull/108)) - ([3c2aee1](https://github.com/johnnybravo-xyz/agd/commit/3c2aee153453d9e47bc29d13d217fa6e7d50e88e))

### 🚜 Refactor


- *(config)* [**breaking**] Improve configuration architecture ([#78](https://github.com/johnnybravo-xyz/agd/pull/78)) - ([4053d2e](https://github.com/johnnybravo-xyz/agd/commit/4053d2e8655125ec712438d021c89c768dd0962b))
- Consolidate MCP into agd - ([45862d3](https://github.com/johnnybravo-xyz/agd/commit/45862d3014d071a1046f57ead5487ec634a6862b))
- Move integration tests to tests/ directory ([#100](https://github.com/johnnybravo-xyz/agd/pull/100)) - ([4cb31b1](https://github.com/johnnybravo-xyz/agd/commit/4cb31b180b6f6b87155a6a9e49c849fb57e1525b))

### 🧪 Testing


- *(core)* Fix flaky config tests with proper env isolation ([#49](https://github.com/johnnybravo-xyz/agd/pull/49)) - ([cbe8077](https://github.com/johnnybravo-xyz/agd/commit/cbe80778a15f930e3ae22541c2eec5020816c48f))

### ⚙️ Miscellaneous Tasks


- Release ([#209](https://github.com/johnnybravo-xyz/agd/pull/209)) - ([2191220](https://github.com/johnnybravo-xyz/agd/commit/2191220252ec94fe78039e2863a73e25e5429c10))
- Bump minor version ([#207](https://github.com/johnnybravo-xyz/agd/pull/207)) - ([f6d2757](https://github.com/johnnybravo-xyz/agd/commit/f6d2757216e314c8697d14536c6ecdd24159ee8b))
- Release v0.0.8 ([#176](https://github.com/johnnybravo-xyz/agd/pull/176)) - ([0838802](https://github.com/johnnybravo-xyz/agd/commit/0838802403bbb93c6ba9607b61bc2b62e2ad3874))
- Migrate from unmaintained paste crate to pastey ([#174](https://github.com/johnnybravo-xyz/agd/pull/174)) - ([6f51563](https://github.com/johnnybravo-xyz/agd/commit/6f515636a1ed943fbfaca950ecdd56a3e09b665e))
- Release v0.0.7 ([#169](https://github.com/johnnybravo-xyz/agd/pull/169)) - ([bf50294](https://github.com/johnnybravo-xyz/agd/commit/bf502948d1879ce4899795428ba741f792059821))
- Release v0.0.7 ([#164](https://github.com/johnnybravo-xyz/agd/pull/164)) - ([4bd5286](https://github.com/johnnybravo-xyz/agd/commit/4bd5286cb8b6e5ebfb0b021c00aa8dc2f999f08d))
- Some fixes to flake and agenda ([#158](https://github.com/johnnybravo-xyz/agd/pull/158)) - ([3d96b65](https://github.com/johnnybravo-xyz/agd/commit/3d96b65fe5be59ab6f1a666c5f15fba7dfa45db9))
- Release v0.0.6 ([#130](https://github.com/johnnybravo-xyz/agd/pull/130)) - ([7a83fe8](https://github.com/johnnybravo-xyz/agd/commit/7a83fe863d5978cffded6fb20eb45116d734ad38))
- Release v0.0.5 ([#98](https://github.com/johnnybravo-xyz/agd/pull/98)) - ([7ad4b79](https://github.com/johnnybravo-xyz/agd/commit/7ad4b793aec226a33854f9220b62921fcef21e81))
- Consolidate test fixtures into test-utils crate ([#99](https://github.com/johnnybravo-xyz/agd/pull/99)) - ([6101d25](https://github.com/johnnybravo-xyz/agd/commit/6101d25baa9429deb8b7343767cdb7a5016fbaec))
- Release v0.0.4 ([#82](https://github.com/johnnybravo-xyz/agd/pull/82)) - ([67954f9](https://github.com/johnnybravo-xyz/agd/commit/67954f90632868a8a65f6b954e659e22e5e4b2cb))
- Release v0.0.3 ([#77](https://github.com/johnnybravo-xyz/agd/pull/77)) - ([1fe414f](https://github.com/johnnybravo-xyz/agd/commit/1fe414ff9c94d783d2956f5e62ddc177dc78d403))
- Release v0.0.2 ([#68](https://github.com/johnnybravo-xyz/agd/pull/68)) - ([dad6fac](https://github.com/johnnybravo-xyz/agd/commit/dad6facd0fe745a90eb4c41accf04ea986583b8a))
- Prepare crate metadata for crates.io publishing ([#67](https://github.com/johnnybravo-xyz/agd/pull/67)) - ([6c59254](https://github.com/johnnybravo-xyz/agd/commit/6c592543f0ffc48420346b7775b0a5a9fdf0ed92))
- Release v0.0.1 ([#66](https://github.com/johnnybravo-xyz/agd/pull/66)) - ([613c0ec](https://github.com/johnnybravo-xyz/agd/commit/613c0ec301cb1efe212b3f1a429e7fdfabdae000))
- Prepare initial release ([#65](https://github.com/johnnybravo-xyz/agd/pull/65)) - ([937cbb1](https://github.com/johnnybravo-xyz/agd/commit/937cbb166d5ee7d8f8698a70632cfe0d572a2f7b))
- Release v0.0.1 ([#63](https://github.com/johnnybravo-xyz/agd/pull/63)) - ([c3b105a](https://github.com/johnnybravo-xyz/agd/commit/c3b105a49aa95752e3efc75f5f4cbd577eee3536))
- Prepare for initial 0.0.1 release ([#62](https://github.com/johnnybravo-xyz/agd/pull/62)) - ([902fcd1](https://github.com/johnnybravo-xyz/agd/commit/902fcd13d9e5055b8902fbdfe49ee324e739cba9))
- Release v0.2.0 ([#60](https://github.com/johnnybravo-xyz/agd/pull/60)) - ([e4eee04](https://github.com/johnnybravo-xyz/agd/commit/e4eee04818a59673512d36657a30454eec333412))

### Other changes


- *(readme)* Simplify ([#217](https://github.com/johnnybravo-xyz/agd/pull/217)) - ([d0dd797](https://github.com/johnnybravo-xyz/agd/commit/d0dd7978cafe9e91c96877915f13c12e5bf29b2c))
- Escape imported fixture paths on Windows - ([37240b8](https://github.com/johnnybravo-xyz/agd/commit/37240b80df6484b6617ba781c250c2e787da2b6f))
- Normalize agenda oracle line endings - ([12b7e83](https://github.com/johnnybravo-xyz/agd/commit/12b7e83f5361ea83553c628de1a150d58e8902c3))
- Fix Windows atomic write completion - ([fa047fd](https://github.com/johnnybravo-xyz/agd/commit/fa047fdb1113892117ef3edd3df9b795ba27e58f))
- Harden release and platform checks - ([1d7c4b4](https://github.com/johnnybravo-xyz/agd/commit/1d7c4b4ba4732fb6e82bfcbaea57fcc31d4e5e4c))
- Document native workflows and release readiness - ([7022a94](https://github.com/johnnybravo-xyz/agd/commit/7022a94748cf10345b95ea9bd8ba199a0121c622))
- Harden final native core behavior - ([0c02bf3](https://github.com/johnnybravo-xyz/agd/commit/0c02bf341d7d2d4261b4a0372365e6d1e2362545))
- Add editor conformance oracles - ([d7c50d4](https://github.com/johnnybravo-xyz/agd/commit/d7c50d4fe9eb0eec68192c7cd463665078f47c1b))
- Implement agenda and transactional writes - ([1b483d4](https://github.com/johnnybravo-xyz/agd/commit/1b483d49192276e3ec9637381636f6beddcf59ff))
- Build lossless org core - ([6f4bfed](https://github.com/johnnybravo-xyz/agd/commit/6f4bfed628651620179d37acb8b592c75db5a998))
- Rebrand project for agd - ([d6c7bce](https://github.com/johnnybravo-xyz/agd/commit/d6c7bcedbc5eb2d6ddd7ef0d0adc07164cb188e5))
- Update configuration sample and path in README ([#120](https://github.com/johnnybravo-xyz/agd/pull/120)) - ([fa9ad89](https://github.com/johnnybravo-xyz/agd/commit/fa9ad89043f4ff2e38703f84daaef72d8c664b35))
- Add cargo install and GitHub releases to setup instructions ([#81](https://github.com/johnnybravo-xyz/agd/pull/81)) - ([2b543ed](https://github.com/johnnybravo-xyz/agd/commit/2b543edfe29cc1d652f9b1880a09eecb4a41f432))
- Convert README from org-mode to markdown ([#72](https://github.com/johnnybravo-xyz/agd/pull/72)) - ([2d7d3ec](https://github.com/johnnybravo-xyz/agd/commit/2d7d3eceec8b1e2f921b796a281f44d94fdaa293))
- Initial commit - ([e827618](https://github.com/johnnybravo-xyz/agd/commit/e8276188477a12a0cee1863844a25b578584768d))

### ⬆️ Dependency updates


- *(deps)* Switch orgize to crates.io 0.10.0-alpha.1 ([#167](https://github.com/johnnybravo-xyz/agd/pull/167)) - ([cec3c60](https://github.com/johnnybravo-xyz/agd/commit/cec3c60d9ca88071e1110124d57a52040e088317))
- *(deps)* Update rust crate tracing-subscriber to v0.3.23 ([#160](https://github.com/johnnybravo-xyz/agd/pull/160)) - ([ad65a54](https://github.com/johnnybravo-xyz/agd/commit/ad65a54c0dc6cd4af4280f8c0572b2ec09717d90))
- *(deps)* Update rust crate toml to 0.9 ([#46](https://github.com/johnnybravo-xyz/agd/pull/46)) - ([9e610d6](https://github.com/johnnybravo-xyz/agd/commit/9e610d626366314dd734d03fabff1c525c0d8755))


### Added

- Added a source-preserving `OrgDocument` parser that honors in-file `#+TODO`
  and `#+SEQ_TODO` declarations and indexes headline source spans.
- Added deterministic path checks for parse stability and duplicate heading
  IDs.
- Added typed item refs, planning occurrences, inherited tags/categories, and
  deterministic agenda ordering with repeaters, ranges, and deadline warnings.
- Added tags/TODO matching, the supported org-ql subset, and
  Emacs-compatible agenda CSV rows.
- Added minimal byte-span mutations for TODO state, planning, metadata,
  capture, and subtree refile.
- Added advisory locks, Emacs dirty-lock detection, fresh ref validation,
  crash-consistent journals, atomic replacement, and guarded undo.
- Added strict rooted discovery, symlink read containment, symlink write
  refusal, and stable machine-readable error responses.
- Added versioned cold import snapshots that cannot broaden configured roots.

## [0.0.1] - 2026-10-13

### Changed

- Published the upstream-derived core as `agd-core` for the `agd` CLI.
- Relicensed the combined distribution under GPL-3.0-or-later while retaining
  the upstream MIT notice.

### Upstream `org-core` history


## [0.2.0](https://github.com/szaffarano/org-mcp-server/compare/org-core-v0.1.0...org-core-v0.2.0)

### ✨ Features


- *(agenda)* Show past-scheduled TODO tasks in today's agenda view ([#208](https://github.com/szaffarano/org-mcp-server/pull/208)) - ([665744b](https://github.com/szaffarano/org-mcp-server/commit/665744b39c9b5f7a6a4230af8e8da384aa584a2b))
- *(org)* Add org-update-todo tool and CLI command for updating TODOs ([#213](https://github.com/szaffarano/org-mcp-server/pull/213)) - ([8316984](https://github.com/szaffarano/org-mcp-server/commit/831698408cfd4faf30046a45e3c452843c97660f))


## [0.1.0](https://github.com/szaffarano/org-mcp-server/compare/org-core-v0.0.7...org-core-v0.1.0)

### ⚙️ Miscellaneous Tasks


- Migrate from unmaintained paste crate to pastey ([#174](https://github.com/szaffarano/org-mcp-server/pull/174)) - ([6f51563](https://github.com/szaffarano/org-mcp-server/commit/6f515636a1ed943fbfaca950ecdd56a3e09b665e))


## [0.0.7](https://github.com/szaffarano/org-mcp-server/compare/org-core-v0.0.6...org-core-v0.0.7)

### ⚙️ Miscellaneous Tasks


- Release v0.0.7 ([#164](https://github.com/szaffarano/org-mcp-server/pull/164)) - ([4bd5286](https://github.com/szaffarano/org-mcp-server/commit/4bd5286cb8b6e5ebfb0b021c00aa8dc2f999f08d))
- Some fixes to flake and agenda ([#158](https://github.com/szaffarano/org-mcp-server/pull/158)) - ([3d96b65](https://github.com/szaffarano/org-mcp-server/commit/3d96b65fe5be59ab6f1a666c5f15fba7dfa45db9))

### ⬆️ Dependency updates


- *(deps)* Switch orgize to crates.io 0.10.0-alpha.1 ([#167](https://github.com/szaffarano/org-mcp-server/pull/167)) - ([cec3c60](https://github.com/szaffarano/org-mcp-server/commit/cec3c60d9ca88071e1110124d57a52040e088317))
- *(deps)* Update rust crate tracing-subscriber to v0.3.23 ([#160](https://github.com/szaffarano/org-mcp-server/pull/160)) - ([ad65a54](https://github.com/szaffarano/org-mcp-server/commit/ad65a54c0dc6cd4af4280f8c0572b2ec09717d90))


## [0.0.6](https://github.com/szaffarano/org-mcp-server/compare/org-core-v0.0.5...org-core-v0.0.6)

### 🐛 Bug Fixes


- Correct method call order in last_day_of_month ([#129](https://github.com/szaffarano/org-mcp-server/pull/129)) - ([8fabc3e](https://github.com/szaffarano/org-mcp-server/commit/8fabc3e889dc6e2021589beadeb7479d6e5fd7cc))


## [0.0.5](https://github.com/szaffarano/org-mcp-server/compare/org-core-v0.0.4...org-core-v0.0.5)

### 🐛 Bug Fixes


- Update integration test ([#117](https://github.com/szaffarano/org-mcp-server/pull/117)) - ([b282630](https://github.com/szaffarano/org-mcp-server/commit/b2826308cff22c5ac424f2a6ea769f885d18718e))
- Update integration test flakiness ([#108](https://github.com/szaffarano/org-mcp-server/pull/108)) - ([3c2aee1](https://github.com/szaffarano/org-mcp-server/commit/3c2aee153453d9e47bc29d13d217fa6e7d50e88e))

### 🚜 Refactor


- Move integration tests to tests/ directory ([#100](https://github.com/szaffarano/org-mcp-server/pull/100)) - ([4cb31b1](https://github.com/szaffarano/org-mcp-server/commit/4cb31b180b6f6b87155a6a9e49c849fb57e1525b))

### ⚙️ Miscellaneous Tasks


- Consolidate test fixtures into test-utils crate ([#99](https://github.com/szaffarano/org-mcp-server/pull/99)) - ([6101d25](https://github.com/szaffarano/org-mcp-server/commit/6101d25baa9429deb8b7343767cdb7a5016fbaec))

### Other changes


- Update configuration sample and path in README ([#120](https://github.com/szaffarano/org-mcp-server/pull/120)) - ([fa9ad89](https://github.com/szaffarano/org-mcp-server/commit/fa9ad89043f4ff2e38703f84daaef72d8c664b35))


## [0.0.4](https://github.com/szaffarano/org-mcp-server/compare/org-core-v0.0.3...org-core-v0.0.4)

### ✨ Features


- Implement agenda view functionality ([#86](https://github.com/szaffarano/org-mcp-server/pull/86)) - ([87e412d](https://github.com/szaffarano/org-mcp-server/commit/87e412dbcf5136972afb74ee0dd97400e3f883a3))

### Other changes


- Add cargo install and GitHub releases to setup instructions ([#81](https://github.com/szaffarano/org-mcp-server/pull/81)) - ([2b543ed](https://github.com/szaffarano/org-mcp-server/commit/2b543edfe29cc1d652f9b1880a09eecb4a41f432))


## [0.0.3](https://github.com/szaffarano/org-mcp-server/compare/org-core-v0.0.2...org-core-v0.0.3)

### ✨ Features


- *(mcp)* Add tags parameter to list files tool ([#79](https://github.com/szaffarano/org-mcp-server/pull/79)) - ([1b02b24](https://github.com/szaffarano/org-mcp-server/commit/1b02b243cd9d3f478cd46b682183e098b408c93f))

### 🚜 Refactor


- *(config)* [**breaking**] Improve configuration architecture ([#78](https://github.com/szaffarano/org-mcp-server/pull/78)) - ([4053d2e](https://github.com/szaffarano/org-mcp-server/commit/4053d2e8655125ec712438d021c89c768dd0962b))


## [0.0.2](https://github.com/szaffarano/org-mcp-server/compare/org-core-v0.0.1...org-core-v0.0.2) - 2025-10-12

### Added

- add tag filtering support for search and list operations ([#71](https://github.com/szaffarano/org-mcp-server/pull/71))

### Other

- convert README from org-mode to markdown ([#72](https://github.com/szaffarano/org-mcp-server/pull/72))
- prepare crate metadata for crates.io publishing ([#67](https://github.com/szaffarano/org-mcp-server/pull/67))

## [0.0.1](https://github.com/szaffarano/org-mcp-server/releases/tag/org-core-v0.0.1) - 2025-10-06

### Added

- add TOML-based configuration system ([#45](https://github.com/szaffarano/org-mcp-server/pull/45))
- Implement search tool ([#30](https://github.com/szaffarano/org-mcp-server/pull/30))
- *(mcp-server)* Rename binary ([#24](https://github.com/szaffarano/org-mcp-server/pull/24))
- *(mcp-server)* Update resource uri ([#23](https://github.com/szaffarano/org-mcp-server/pull/23))
- *(test)* Improve code coverage ([#20](https://github.com/szaffarano/org-mcp-server/pull/20))
- Add coverage ([#11](https://github.com/szaffarano/org-mcp-server/pull/11))
- *(mcp-server)* Add root flag ([#7](https://github.com/szaffarano/org-mcp-server/pull/7))

### Fixed

- *(deps)* update rust crate rmcp to 0.8.0 ([#42](https://github.com/szaffarano/org-mcp-server/pull/42))
- *(deps)* update rust crate rowan to 0.16 ([#18](https://github.com/szaffarano/org-mcp-server/pull/18))

### Other

- Prepare initial release ([#65](https://github.com/szaffarano/org-mcp-server/pull/65))
- release v0.0.1 ([#63](https://github.com/szaffarano/org-mcp-server/pull/63))
- rename mcp-server crate to org-mcp-server ([#64](https://github.com/szaffarano/org-mcp-server/pull/64))
- prepare for initial 0.0.1 release ([#62](https://github.com/szaffarano/org-mcp-server/pull/62))
- release v0.2.0 ([#60](https://github.com/szaffarano/org-mcp-server/pull/60))
- *(core)* fix flaky config tests with proper env isolation ([#49](https://github.com/szaffarano/org-mcp-server/pull/49))
- *(deps)* update rust crate toml to 0.9 ([#46](https://github.com/szaffarano/org-mcp-server/pull/46))
- *(docs)* Update readme ([#33](https://github.com/szaffarano/org-mcp-server/pull/33))
- Initial commit
