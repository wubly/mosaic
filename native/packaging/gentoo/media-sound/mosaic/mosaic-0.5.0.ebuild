# Copyright 2026 Gentoo Authors
# Distributed under the terms of the MIT license

EAPI=8

# Generate CRATES with: pycargoebuild native/
CRATES=""

inherit cargo desktop xdg

DESCRIPTION="SoundCloud and your own music in one player"
HOMEPAGE="https://github.com/wubly/mosaic"
SRC_URI="
	https://github.com/wubly/mosaic/archive/refs/tags/v${PV}.tar.gz -> ${P}.tar.gz
	${CARGO_CRATE_URIS}
"
S="${WORKDIR}/mosaic-${PV}/native"

LICENSE="MIT"
# Dependent crate licenses
LICENSE+=" Apache-2.0 BSD ISC MPL-2.0 Unicode-3.0 ZLIB"
SLOT="0"
KEYWORDS="~amd64"
IUSE="+spotify"

RDEPEND="media-libs/alsa-lib"
DEPEND="${RDEPEND}"
BDEPEND="virtual/pkgconfig"

src_configure() {
	local myfeatures=( $(usev spotify) )
	cargo_src_configure --no-default-features
}

src_compile() {
	cargo_src_compile -p mosaic
}

src_install() {
	dobin "$(cargo_target_dir)/mosaic"
	domenu packaging/linux/app.mosaic.player.desktop
	insinto /usr/share/metainfo
	doins packaging/linux/app.mosaic.player.metainfo.xml
	newicon -s scalable assets/icon.svg app.mosaic.player.svg
	newicon -s 512 assets/icon.png app.mosaic.player.png
}
