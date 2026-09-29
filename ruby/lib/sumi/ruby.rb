# frozen_string_literal: true

require_relative "ruby/version"
require_relative "ruby/platforms"

module Sumi
  module Ruby
    class Error < StandardError; end
    class UnsupportedPlatformError < Error; end
    class ExecutableNotFoundError < Error; end
    class DirectoryNotFoundError < Error; end

    GEM_NAME = "sumi-ruby"
    INSTALL_DIR_ENV = "SUMI_INSTALL_DIR"
    DEFAULT_DIR = File.expand_path(File.join(__dir__, "..", "..", "exe"))

    class << self
      # Returns the absolute path of the sumi executable.
      #
      # Native gems carry the executable in `exe/<platform>/`. Setting `SUMI_INSTALL_DIR` to a
      # directory that contains `sumi` (or `sumi.exe`) uses that executable instead, for
      # example on a platform without a native gem.
      def executable(exe_path: DEFAULT_DIR)
        install_dir = ENV[INSTALL_DIR_ENV]
        if install_dir && !install_dir.empty?
          unless File.directory?(install_dir)
            raise DirectoryNotFoundError, "#{INSTALL_DIR_ENV} is set to #{install_dir}, but that directory does not exist"
          end

          file = executable_in(File.expand_path(install_dir))
          return file if file

          raise ExecutableNotFoundError, "#{INSTALL_DIR_ENV} is set to #{install_dir}, but it does not contain a sumi executable"
        end

        file = Dir.glob(File.join(exe_path, "*")).sort.find do |dir|
          File.directory?(dir) && platform_matches?(File.basename(dir)) && executable_in(dir)
        end
        return executable_in(file) if file

        platform = PLATFORMS.keys.find { |name| platform_matches?(name) }
        unless platform
          raise UnsupportedPlatformError, <<~MESSAGE
            #{GEM_NAME} does not ship a sumi executable for #{Gem::Platform.local}.
            Supported platforms: #{PLATFORMS.keys.join(", ")}.
            Build sumi from source (https://github.com/champierre/sumi) and set #{INSTALL_DIR_ENV} to the directory that contains it.
          MESSAGE
        end

        raise ExecutableNotFoundError, <<~MESSAGE
          Cannot find the sumi executable for #{Gem::Platform.local} in #{exe_path}.
          Bundler may have installed the platform-independent #{GEM_NAME} gem. Add the platform to the lockfile and install again:
            bundle lock --add-platform #{platform}
            bundle install
          Or set #{INSTALL_DIR_ENV} to a directory that contains the sumi executable.
        MESSAGE
      end

      private

      def executable_in(dir)
        ["sumi", "sumi.exe"]
          .map { |name| File.join(dir, name) }
          .find { |path| File.file?(path) && File.executable?(path) }
      end

      def platform_matches?(name)
        Gem::Platform.match_gem?(Gem::Platform.new(name), GEM_NAME)
      end
    end
  end
end
