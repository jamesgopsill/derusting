if [ -z "$1" ]; then
  echo "Usage: $0 --bootloader [yes|no]"
  exit 1
fi

if [ -z "$2" ]; then
  echo "Usage: $0 --bootloader [yes|no]"
  exit 1
fi

if [[ "$2" != "yes" && "$2" != "no" ]]; then
  echo "Usage: $0 --bootloader [yes|no]"
  exit 1
fi

cargo build --release || {
  echo "Cargo Build Failed"
  exit 1
}

RLIB=libderusting
TARGET_DIR=$(cargo metadata --format-version 1 --no-deps | jq -r '.target_directory')

echo "Copying Rust lib folder..."
cp -r "./${RLIB}" ./buddy/src

cp "${TARGET_DIR}/thumbv7em-none-eabihf/release/${RLIB}.a" "./buddy/src/${RLIB}"

echo "Patching files..."

CMAKE_LIST=buddy/src/CMakeLists.txt

if ! grep -q "add_subdirectory(${RLIB})" ${CMAKE_LIST}; then
  echo "Patching ${CMAKE_LIST}"
  echo "add_subdirectory(${RLIB})" | cat - ${CMAKE_LIST} >tmp.cpp && mv tmp.cpp ${CMAKE_LIST}
fi

BUDDY_MAIN=buddy/src/buddy/main.cpp

if ! grep -q "#include <${RLIB}/${RLIB}.hpp>" ${BUDDY_MAIN}; then
  echo "Patching ${BUDDY_MAIN} (Include)"
  echo "#include <${RLIB}/${RLIB}.hpp>" | cat - ${BUDDY_MAIN} >tmp.cpp && mv tmp.cpp ${BUDDY_MAIN}
fi

if ! grep -q "derusting_main();" ${BUDDY_MAIN}; then
  echo "Patching ${BUDDY_MAIN} (Function Call)"
  sed -i '/metrics_reconfigure();/a \      derusting_main();' ${BUDDY_MAIN}
fi

NEW_HEAP_SIZE=40960 # 61440 (40960 - Original)
NEW_LINE="#define configTOTAL_HEAP_SIZE ((size_t)$NEW_HEAP_SIZE)"
RTOS_CONFIG=buddy/include/stm32f4_hal/FreeRTOSConfig.h
if grep -qF "$NEW_LINE" "$RTOS_CONFIG"; then
  echo "Heap size is already $NEW_HEAP_SIZE. Skipping update to prevent rebuild."
else
  sed -i "s/^#define configTOTAL_HEAP_SIZE.*/#define configTOTAL_HEAP_SIZE ((size_t)$NEW_HEAP_SIZE)/" "${RTOS_CONFIG}"
  grep "configTOTAL_HEAP_SIZE" "${RTOS_CONFIG}"
fi

# UI elements
rsync -c assets/screen_home.hpp buddy/src/gui/screen_home.hpp
rsync -c assets/screen_home.cpp buddy/src/gui/screen_home.cpp

cd ./buddy || {
  echo "Failed to find buddy dir"
}

if [[ "$2" == "yes" ]]; then {
  python utils/build.py --preset mini --build-type release --bootloader yes \
    -DWUI:STRING=YES \
    -DBUDDY_ENABLE_WUI:BOOL=YES \
    -DCONNECT:STRING=NO \
    -DBUDDY_ENABLE_CONNECT:BOOL=OFF \
    -DHAS_NFC:BOOL=OFF
}; else
  {
    python utils/build.py --preset mini --build-type release --bootloader no \
      -DWUI:STRING=YES \
      -DBUDDY_ENABLE_WUI:BOOL=YES \
      -DCONNECT:STRING=NO \
      -DBUDDY_ENABLE_CONNECT:BOOL=OFF \
      -DHAS_NFC:BOOL=OFF
  }
fi

cd ..

echo "BUILD FINISHED"
