# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.1](https://github.com/sound-systems/fileloft/compare/fileloft-store-fs-v0.3.0...fileloft-store-fs-v0.3.1) - 2026-08-02

### Fixed

- *(security)* reject upload IDs that collide with backend object keys ([#8](https://github.com/sound-systems/fileloft/pull/8))

## [0.3.0](https://github.com/sound-systems/fileloft/compare/fileloft-store-fs-v0.2.1...fileloft-store-fs-v0.3.0) - 2026-04-25

### Fixed

- security audit with corresponding fixes and enforcement tooling

## [0.2.1](https://github.com/sound-systems/fileloft/compare/fileloft-store-fs-v0.2.0...fileloft-store-fs-v0.2.1) - 2026-04-25

### Other

- setup docker builds per supported environment ([#4](https://github.com/sound-systems/fileloft/pull/4))

## [0.2.0](https://github.com/sound-systems/fileloft/compare/fileloft-store-fs-v0.1.0...fileloft-store-fs-v0.2.0) - 2026-04-09

### Added

- parity with tus standard config options
- docker builds for supported storage backends
- *(fs)* align FileStore with tusd/GCS key layout

### Fixed

- ensure e2e tests are setup correctly with chromedriver version match
