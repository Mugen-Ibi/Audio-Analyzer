# Distribution of the ASIO-enabled release

The ASIO-enabled Audio Visualizer executable and its corresponding combined work
are conveyed under **GNU GPL version 3**. See `COPYING.GPLv3`. This selection uses
the GPLv3 alternative offered by the ASIO SDK used for this release.

The project's original MIT grant in `LICENSE` is retained for the original project
code. Copyright and license notices belonging to dependencies and fonts are also
retained; their individual files continue to carry those terms. The complete
ASIO-enabled executable is distributed under GPLv3, rather than solely under the
project's original MIT grant. No proprietary Steinberg agreement is claimed.

The release supplies a corresponding-source ZIP beside the Windows executable
and binary ZIP, at no additional charge. It contains the exact tagged project
source, locked dependency source, the ASIO SDK source needed by this Windows
build, license texts, an offline Cargo source configuration and the build script.
Ordinary unmodified build tools and system libraries (Rust/Cargo, MSVC, Windows
SDK and LLVM/libclang) are prerequisites and are not included.

The source ZIP's `SOURCE_PROVENANCE.json` identifies the product commit and build
configuration. `SOURCE-MANIFEST.sha256` identifies its files. The binary ZIP's
`BUILD-PROVENANCE.json` and the release `SHA256SUMS.txt` identify the compiled
asset. The automatically generated GitHub source archive alone does not include
the separately supplied dependency/ASIO source, so use the corresponding-source
asset for the complete offline build inputs.

ASIO is a trademark of Steinberg Media Technologies GmbH. No endorsement by
Steinberg is claimed. Original notices in the SDK sources are preserved; the
GPLv3 alternative is selected for SDK files referring to its root license, while
the host helper files retain their embedded BSD-style notices.

There is no warranty, to the extent permitted by applicable law. You may copy,
modify and redistribute under GPLv3. See its full terms in `COPYING.GPLv3`.
