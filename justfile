default:
	just --list

# Generate documentation for default feature set.
docs *EXTRA:
	cargo doc {{EXTRA}}

# Generate documentation for default feature set.
docs-nightly *EXTRA:
	RUSTDOCFLAGS='--cfg=docsrs' cargo +nightly doc {{EXTRA}}

# Generate documentation for all features.
docs-nightly-all *EXTRA:
	RUSTDOCFLAGS='--cfg=docsrs' cargo +nightly doc --all-features {{EXTRA}}

# Generate documentation for minimal feature set.
docs-min *EXTRA:
	cargo doc --no-default-features {{EXTRA}}

# Run tests with all features.
test *EXTRA:
	cargo test --all-features {{EXTRA}}

# Run tests using miri
test-miri *EXTRA:
	cargo miri test {{EXTRA}}

# Check all features and targets
check:
	cargo clippy --all-features --all-targets --workspace

install:
	cargo +nightly install --path . -Z build-std=std,panic_abort -Z build-std-features="optimize_for_size"

build *EXTRA:
	cargo +nightly build --release -Z build-std=std,panic_abort -Z build-std-features="optimize_for_size" {{EXTRA}}
