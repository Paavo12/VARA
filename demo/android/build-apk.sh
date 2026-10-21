#!/usr/bin/env bash
# Offline APK build for the FUber wrapper app: no Gradle and no downloads. It uses only the
# Android SDK build-tools and Android Studio's bundled JDK.
#
#   ./build-apk.sh            build  build/hera-demo.apk
#   ./build-apk.sh install    build, then install on the running emulator/phone
#
# Override paths with SDK=..., JDK=..., BT=..., PLATFORM=... if yours differ.
set -euo pipefail
cd "$(dirname "$0")"

SDK="${SDK:-/d/Android/Sdk}"
JDK="${JDK:-/d/Android/AndroidStudio/jbr}"
BT="${BT:-$SDK/build-tools/36.0.0}"
PLATFORM="${PLATFORM:-$SDK/platforms/android-35/android.jar}"
OUT=build
MAIN=app/src/main
export JAVA_HOME="$JDK" PATH="$JDK/bin:$PATH" # d8 and apksigner are Java tools
LAN_URL="$(grep -E '^heraLanUrl=' gradle.properties | cut -d= -f2- || true)"

rm -rf "$OUT" && mkdir -p "$OUT/res" "$OUT/gen/app/hera/demo" "$OUT/classes" "$OUT/dex"

echo "› compiling resources"
"$BT/aapt2.exe" compile --dir "$MAIN/res" -o "$OUT/res"

echo "› linking manifest and resources"
# Gradle takes the package name from build.gradle.kts; aapt2 needs it in the manifest itself.
sed 's#<manifest #<manifest package="app.hera.demo" #' "$MAIN/AndroidManifest.xml" > "$OUT/AndroidManifest.xml"
"$BT/aapt2.exe" link -o "$OUT/unsigned.apk" -I "$PLATFORM" --manifest "$OUT/AndroidManifest.xml" \
  --min-sdk-version 26 --target-sdk-version 35 --version-code 1 --version-name 1.0 \
  "$OUT"/res/*.flat

echo "› bundling web app"
# Added with jar rather than aapt2 -A: on Windows aapt2 writes backslashes into asset paths.
mkdir -p "$OUT/stage/assets" && cp -r ../public/. "$OUT/stage/assets/"
"$JDK/bin/jar.exe" uf "$OUT/unsigned.apk" -C "$OUT/stage" assets

echo "› compiling Java"
cat > "$OUT/gen/app/hera/demo/BuildConfig.java" <<EOF
package app.hera.demo;
public final class BuildConfig {
    public static final String LAN_URL = "$LAN_URL";
}
EOF
"$JDK/bin/javac.exe" -nowarn --release 17 -classpath "$PLATFORM" -d "$OUT/classes" \
  $(find "$MAIN/java" "$OUT/gen" -name '*.java')

echo "› dexing"
"$BT/d8.bat" --min-api 26 --lib "$PLATFORM" --output "$OUT/dex" $(find "$OUT/classes" -name '*.class')
(cd "$OUT/dex" && "$JDK/bin/jar.exe" uf ../unsigned.apk classes.dex)

echo "› aligning and signing"
KS=debug.keystore
if [ ! -f "$KS" ]; then
  "$JDK/bin/keytool.exe" -genkeypair -keystore "$KS" -storepass android -keypass android -alias androiddebugkey \
    -keyalg RSA -keysize 2048 -validity 10000 -dname "CN=FUber Demo Debug" >/dev/null 2>&1
fi
"$BT/zipalign.exe" -f -p 4 "$OUT/unsigned.apk" "$OUT/aligned.apk"
"$BT/apksigner.bat" sign --ks "$KS" --ks-pass pass:android --key-pass pass:android \
  --out "$OUT/hera-demo.apk" "$OUT/aligned.apk"
echo "✓ $OUT/hera-demo.apk ($(du -h "$OUT/hera-demo.apk" | cut -f1))"

if [ "${1:-}" = "install" ]; then
  # Installs on every running emulator/phone (plain adb refuses when more than one is connected).
  ADB="$SDK/platform-tools/adb.exe"
  DEVICES="$("$ADB" devices | tr -d '\r' | awk 'NR > 1 && $2 == "device" { print $1 }')"
  [ -n "$DEVICES" ] || { echo "✗ no emulator or phone connected"; exit 1; }
  for dev in $DEVICES; do
    echo "› installing on $dev"
    "$ADB" -s "$dev" install -r "$OUT/hera-demo.apk"
    "$ADB" -s "$dev" reverse tcp:3100 tcp:3100 || true
  done
  echo "✓ installed on $(echo "$DEVICES" | wc -w) device(s) — look for 'FUber Rider' and 'FUber Driver' in the app drawer"
fi
