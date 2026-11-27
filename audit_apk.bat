@echo off
set APK_PATH=C:\Users\qixua\Desktop\OCN\android\app\build\outputs\apk\debug\app-debug.apk
echo APK Path: %APK_PATH%
if exist %APK_PATH% (
    echo APK exists.
) else (
    echo APK NOT found.
)

:: List entries in APK
echo.
echo APK contents (filtered for lib):
powershell -command "$z = [System.IO.Compression.ZipFile]::OpenRead('%APK_PATH%'); foreach ($e in $z.Entries) { if ($e.FullName -like '*lib*') { Write-Host $e.FullName } }; $z.Dispose()"

:: Check for AndroidManifest.xml inside
echo.
echo Checking AndroidManifest.xml:
if exist android\app\src\main\AndroidManifest.xml (
    echo AndroidManifest.xml exists locally.
) else (
    echo AndroidManifest.xml NOT found locally.
)

:: Check jniLibs
echo.
echo Checking jniLibs\arm64-v8a:
if exist android\app\src\main\jniLibs\arm64-v8a\libocn_core.so (
    echo libocn_core.so exists in jniLibs.
) else (
    echo libocn_core.so NOT found in jniLibs.
)

:: Get APK package name using aapt if available
echo.
echo Attempting to get package name:
for /f "tokens=*" %%I in ('cmd /c "aapt dump badging '%APK_PATH%' 2>nul"') do (
    echo %%I
)

pause