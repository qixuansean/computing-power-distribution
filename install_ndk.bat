@echo off
set JAVA_HOME=C:\Users\qixua\AppData\Local\Android\Sdk\jdk-17
set PATH=C:\Users\qixua\AppData\Local\Android\Sdk\jdk-17\bin;%PATH%
(for /L %%i in (1,1,50) do @echo y) | C:\Users\qixua\AppData\Local\Android\Sdk\cmdline-tools\latest\bin\sdkmanager.bat --sdk_root=C:\Users\qixua\AppData\Local\Android\Sdk "ndk;26.1.10909125" "platforms;android-34" "build-tools;34.0.0"
