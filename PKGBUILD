# Maintainer: YOUR NAME <YOUR EMAIL>
pkgname=habit-tui
pkgver=0.2.0
pkgrel=1
pkgdesc='Local-first terminal habit tracker with weekly, monthly, graph, and totals views'
arch=('x86_64' 'aarch64')
url='https://github.com/illicit-sly-git/habit-tui'
license=('MIT')
depends=('gcc-libs')
makedepends=('cargo' 'rust')
source=("git+https://github.com/illicit-sly-git/habit-tui.git#tag=v${pkgver}")
sha256sums=('SKIP')

prepare() {
  cd "$srcdir/$pkgname"
  # Cargo.lock should be committed to the release tag for reproducible builds.
  test -f Cargo.lock || {
    error 'Cargo.lock is missing. Run cargo generate-lockfile, commit it, and retag the release.'
    return 1
  }
  cargo fetch --locked
}

build() {
  cd "$srcdir/$pkgname"
  cargo build --release --locked --offline
}

check() {
  cd "$srcdir/$pkgname"
  cargo test --release --locked --offline
}

package() {
  cd "$srcdir/$pkgname"
  install -Dm755 "target/release/$pkgname" "$pkgdir/usr/bin/$pkgname"
  install -Dm644 LICENSE "$pkgdir/usr/share/licenses/$pkgname/LICENSE"
  install -Dm644 README.md "$pkgdir/usr/share/doc/$pkgname/README.md"
}
