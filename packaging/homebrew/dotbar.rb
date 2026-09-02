# Formula for the tlehman/homebrew-tap repository. Copy it to that repo as
# Formula/dotbar.rb — a tap is a git repo named homebrew-<name>, so
# `brew install tlehman/tap/dotbar` resolves to github.com/tlehman/homebrew-tap.
#
# On each release: bump url to the new tag and recompute the sha256 with
#   curl -sL https://github.com/tlehman/dotbar/archive/refs/tags/vX.Y.Z.tar.gz | shasum -a 256
class Dotbar < Formula
  desc "Braille-dot progress bar for statuslines and terminals"
  homepage "https://github.com/tlehman/dotbar"
  url "https://github.com/tlehman/dotbar/archive/refs/tags/v0.2.0.tar.gz"
  sha256 "9bbeb1b852cb39960bcc5e84770bc0cb634de3cb2530f05cdef88b13c4b63ce0"
  license any_of: ["MIT", "Apache-2.0"]
  head "https://github.com/tlehman/dotbar.git", branch: "main"

  depends_on "rust" => :build

  def install
    system "cargo", "install", *std_cargo_args
  end

  test do
    # 76% renders 13 cells; NO_COLOR keeps the comparison to literal text.
    assert_match "76%", shell_output("NO_COLOR=1 #{bin}/dotbar 76")
    # No such field means no output and exit 0, not an error.
    assert_equal "", shell_output("echo '{}' | #{bin}/dotbar")
  end
end
