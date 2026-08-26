if bash build_firmware.sh 2>&1 | tee /dev/stderr | grep -q "SUCCESS"; then
  bash flash.sh
fi
