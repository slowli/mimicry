# Changelog

All notable changes to this project will be documented in this file.
The project adheres to [Semantic Versioning](http://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Support passing values to `Answers` via a message channel. This allows
  controlling when the answers are consumed and specifying answers after
  the mock is set.
- Support mocking async functions / methods.

### Changed

- Change `call_real()` / `call_real_once()` interface. Now, these methods return
  a guard that can then be used on its own or using `scope()` / `async_scope()` wrappers.
- Bump minimum supported Rust version from 1.57 to 1.59.

## 0.1.0 - 2022-07-04

The initial release of `mimicry`.
