set -euxo pipefail

main() {
    cargo doc --features 'maybe-uninit union'

    mkdir ghp-import
    curl -Ls https://github.com/davisp/ghp-import/archive/master.tar.gz |
        tar --strip-components 1 -C ghp-import -xz

    ./ghp-import/ghp_import.py target/$TARGET/doc

    set +x
    git push -fq https://$GH_TOKEN@github.com/$TRAVIS_REPO_SLUG.git gh-pages && echo OK
}

if [ $TRAVIS_BRANCH = master ] && [ $TRAVIS_PULL_REQUEST = false ] && [ $TARGET = thumbv7m-none-eabi ]; then
    main
fi
