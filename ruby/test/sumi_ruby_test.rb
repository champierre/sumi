# frozen_string_literal: true

require "minitest/autorun"
require "fileutils"
require "tmpdir"
require "sumi/ruby"

class SumiRubyTest < Minitest::Test
  def setup
    @install_dir = ENV.delete(Sumi::Ruby::INSTALL_DIR_ENV)
  end

  def teardown
    if @install_dir
      ENV[Sumi::Ruby::INSTALL_DIR_ENV] = @install_dir
    else
      ENV.delete(Sumi::Ruby::INSTALL_DIR_ENV)
    end
  end

  def test_version_matches_the_crate_version
    cargo = File.read(File.expand_path("../../Cargo.toml", __dir__))
    assert_equal cargo[/^version = "(.+)"/, 1], Sumi::Ruby::VERSION
  end

  def test_platform_names_are_canonical
    # Older RubyGems drop the "gnu" of "x86_64-linux-gnu"; the Linux gems require 3.3.22.
    skip "RubyGems #{Gem::VERSION} predates linux-gnu platforms" if Gem::Version.new(Gem::VERSION) < Gem::Version.new("3.3.22")
    Sumi::Ruby::PLATFORMS.each_key do |name|
      assert_equal name, Gem::Platform.new(name).to_s
    end
  end

  def test_finds_the_executable_for_this_platform
    Dir.mktmpdir do |dir|
      other = Sumi::Ruby::PLATFORMS.keys.find { |p| !matches?(p) }
      fake_executable(File.join(dir, other))
      expected = fake_executable(File.join(dir, local_platform))
      assert_equal expected, Sumi::Ruby.executable(exe_path: dir)
    end
  end

  def test_missing_executable_is_an_error
    local_platform
    Dir.mktmpdir do |dir|
      error = assert_raises(Sumi::Ruby::ExecutableNotFoundError) { Sumi::Ruby.executable(exe_path: dir) }
      assert_includes error.message, "bundle lock --add-platform"
    end
  end

  def test_install_dir_overrides_the_bundled_executable
    Dir.mktmpdir do |dir|
      expected = fake_executable(dir)
      ENV[Sumi::Ruby::INSTALL_DIR_ENV] = dir
      assert_equal expected, Sumi::Ruby.executable(exe_path: File.join(dir, "missing"))
    end
  end

  def test_install_dir_must_exist_and_contain_sumi
    Dir.mktmpdir do |dir|
      ENV[Sumi::Ruby::INSTALL_DIR_ENV] = File.join(dir, "missing")
      assert_raises(Sumi::Ruby::DirectoryNotFoundError) { Sumi::Ruby.executable }
      ENV[Sumi::Ruby::INSTALL_DIR_ENV] = dir
      assert_raises(Sumi::Ruby::ExecutableNotFoundError) { Sumi::Ruby.executable }
    end
  end

  def test_convert_passes_the_pdf_and_options_to_sumi
    skip "the fake executable is a shell script" if Gem.win_platform?
    Dir.mktmpdir do |dir|
      fake_executable(dir, <<~SH)
        echo "$@"
        cat
      SH
      ENV[Sumi::Ruby::INSTALL_DIR_ENV] = dir
      assert_equal "- -o - --gray-model colorimetric --strict --timeout 30\n%PDF-1.7",
                   Sumi.convert("%PDF-1.7", gray_model: "colorimetric", strict: true, overwrite: false, timeout: 30)
    end
  end

  def test_convert_raises_sumi_errors
    skip "the fake executable is a shell script" if Gem.win_platform?
    Dir.mktmpdir do |dir|
      fake_executable(dir, <<~SH)
        echo "error: not a PDF" >&2
        exit 1
      SH
      ENV[Sumi::Ruby::INSTALL_DIR_ENV] = dir
      error = assert_raises(Sumi::ConversionError) { Sumi.convert("not a PDF") }
      assert_equal "error: not a PDF", error.message
    end
  end

  def test_convert_reports_the_exit_status_without_error_output
    skip "the fake executable is a shell script" if Gem.win_platform?
    Dir.mktmpdir do |dir|
      fake_executable(dir, "exit 3\n")
      ENV[Sumi::Ruby::INSTALL_DIR_ENV] = dir
      error = assert_raises(Sumi::ConversionError) { Sumi.convert("%PDF-1.7") }
      assert_equal "sumi exited with status 3", error.message
    end
  end

  private

  def local_platform
    Sumi::Ruby::PLATFORMS.keys.find { |p| matches?(p) } || skip("no native gem for #{Gem::Platform.local}")
  end

  def matches?(platform)
    Gem::Platform.match_gem?(Gem::Platform.new(platform), Sumi::Ruby::GEM_NAME)
  end

  def fake_executable(dir, script = nil)
    FileUtils.mkdir_p(dir)
    path = File.join(dir, Gem.win_platform? ? "sumi.exe" : "sumi")
    File.write(path, script ? "#!/bin/sh\n#{script}" : "")
    File.chmod(0o755, path)
    path
  end
end
