Pod::Spec.new do |s|
  s.name             = 'ores_common_mobile_host'
  s.version          = '0.1.0'
  s.summary          = 'Shared ORES Flutter mobile hosting and background wake bridge.'
  s.description      = <<-DESC
Shared platform bridge for ORES mobile hosting, including iOS wake-and-drain BackgroundTasks integration.
                       DESC
  s.homepage         = 'https://github.com/ORESoftware/ores-common-desktop-infra'
  s.license          = { :type => 'MIT' }
  s.author           = { 'ORESoftware' => 'alex@oresoftware.com' }
  s.source           = { :path => '.' }
  s.source_files     = 'ores_common_mobile_host/Sources/ores_common_mobile_host/**/*.swift'
  s.dependency 'Flutter'
  s.platform         = :ios, '13.0'
  s.swift_version    = '5.9'
end
