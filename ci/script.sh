set -euxo pipefail

main() {
    cargo check --target $TARGET
    if [ $TRAVIS_RUST_VERSION = nightly ]; then
        cargo check --target $TARGET --features 'union'
        cargo check --target $TARGET --features 'asm'
        cargo check --target $TARGET --features 'asm union'
    fi

    if [ $TARGET = x86_64-unknown-linux-gnu ]; then
        cargo test --target $TARGET
        cargo test --target $TARGET --release

        if [ $TRAVIS_RUST_VERSION = nightly ]; then
            cargo test --target $TARGET --features 'union'
            cargo test --target $TARGET --release --features 'union'

            export RUSTFLAGS="-Z sanitizer=address"
            export ASAN_OPTIONS="detect_odr_violation=0"

            cargo test --target $TARGET
            cargo test --target $TARGET --release
        fi
    fi
}

# fake Travis variables to be able to run this on a local machine
if [ -z ${TRAVIS_BRANCH-} ]; then
    TRAVIS_BRANCH=auto
fi

if [ -z ${TRAVIS_PULL_REQUEST-} ]; then
    TRAVIS_PULL_REQUEST=false
fi

if [ -z ${TRAVIS_RUST_VERSION-} ]; then
    case $(rustc -V) in
        *nightly*)
            TRAVIS_RUST_VERSION=nightly
            ;;
        *beta*)
            TRAVIS_RUST_VERSION=beta
            ;;
        *)
            TRAVIS_RUST_VERSION=stable
            ;;
    esac
fi

if [ -z ${TARGET-} ]; then
    TARGET=$(rustc -Vv | grep host | cut -d ' ' -f2)
fi

if [ $TRAVIS_BRANCH != master ] || [ $TRAVIS_PULL_REQUEST != false ]; then
    main
fi
