if bash build_firmware.sh --bootloader no 2>&1 | tee /dev/stderr | grep -q "SUCCESS"; then
  bash flash.sh
fi
