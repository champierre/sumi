# frozen_string_literal: true

require "open3"

module Sumi
  class ConversionError < Ruby::Error; end

  class << self
    # Converts the PDF in `pdf` (a String of its bytes) and returns the converted PDF as a String.
    #
    # Each keyword becomes a sumi option: `gray_model: "colorimetric"` is passed as
    # `--gray-model colorimetric`, `strict: true` as `--strict`, and `false` or `nil` leaves the
    # option out. The keywords are not checked here, so options added to sumi can be used without
    # updating this gem; sumi itself rejects unknown ones.
    #
    # Raises ConversionError with sumi's error output when sumi exits with a non-zero status.
    def convert(pdf, **options)
      output, error, status = Open3.capture3(Ruby.executable, "-", "-o", "-", *command_options(options),
                                             stdin_data: pdf, binmode: true)
      unless status.success?
        message = error.strip
        message = "sumi exited with status #{status.exitstatus}" if message.empty?
        raise ConversionError, message
      end

      output
    end

    private

    def command_options(options)
      options.flat_map do |name, value|
        flag = "--#{name.to_s.tr("_", "-")}"
        case value
        when true then [flag]
        when false, nil then []
        else [flag, value.to_s]
        end
      end
    end
  end
end
