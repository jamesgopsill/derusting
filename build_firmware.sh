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

cd ./buddy || {
  echo "Failed to find buddy dir"
}

#python utils/build.py --preset mini --build-type release --bootloader no
python utils/build.py --preset mini --build-type release --bootloader no \
  -DWUI:STRING=YES \
  -DBUDDY_ENABLE_WUI:BOOL=YES \
  -DCONNECT:STRING=NO \
  -DBUDDY_ENABLE_CONNECT:BOOL=OFF \
  -DHAS_NFC:BOOL=OFF

cd ..

echo "BUILD FINISHED"
