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
    # NO_COLOR keeps the SGR sequences out of the comparison; the bar itself is
    # 13 cells at 1% per dot, so 50% is six full cells plus a two-dot cell.
    assert_equal "⣿⣿⣿⣿⣿⣿⡄⣀⣀⣀⣀⣀⣀ 50%",
                 shell_output("NO_COLOR=1 #{bin}/dotbar 50").chomp
    assert_equal "⣿⡿⣀ 76%",
                 shell_output("NO_COLOR=1 #{bin}/dotbar --dense 76").chomp
    # Statusline mode: renders 100 - remaining_percentage from stdin.
    assert_match(/ 76%$/,
                 pipe_output("NO_COLOR=1 #{bin}/dotbar",
                             '{"context_window":{"remaining_percentage":24.3}}'))
    # An unknown subcommand is dispatched, not silently ignored.
    assert_match "is not a dotbar command",
                 shell_output("#{bin}/dotbar no-such-helper 2>&1", 1)
  end
end
