probe-rs run --chip STM32F407VG ./buddy/build/mini_release_noboot/firmware

# probe-rs download --chip STM32F407VGTx --binary-format bin --base-address 0x08000000 ./buddy/.dependencies/bootloader-mini-2.6.0/bootloader.bin
# probe-rs download --chip STM32F407VGTx --binary-format bin --base-address 0x08020000 ./buddy/build/mini_release_boot/firmware.bin
# probe-rs reset --chip STM32F407VGTx

# probe-rs download --chip STM32F407VGTx ./buddy/build/mini_release_boot/firmware
# probe-rs reset --chip STM32F407VGTx
# probe-rs attach --chip STM32F407VGTx ./buddy/build/mini_release_boot/firmware
