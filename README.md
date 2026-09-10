# derusting - **De**centralised T**rust**ed Manufactur**ing** in **Rust**.

Derusting is a **de**centralised t**rust**ed manufactur**ing** system demonstrator written in **Rust** and patched into the Prusa Buddy Firmware. The demonstrator turns any of their Additive Manufacturing machines into decentralised agents that seek each other out on a network to form their own manufacturing collectives. The objective is to showcase and alternative to centralised systems which have challenges concerning single-point of failures, centralised control and governance and ownership of intellectural property. Decentralised systems can mitigate much of these concerns by empowering the very machines to manage and negotiate the jobs they manufacture in a trusted secure, resilient and responsive environment!

![Lab](https://github.com/jamesgopsill/derusting_book/blob/main/src/assets/lab.png?raw=true)

The demonstrator has an accompanying book and YouTube series to get you started with the principles of building decentralised systems. Please feel free to reach out if you want to learn more or would like a hand!

## Getting Started

You can jump right in by using a pre-compiled firmware for the Prusa Mini or adapt and build your own using the development steps further below. Here, we show you how to get started with the pre-compiled version. And don't worry about needing lots of printers. A de-centralised service can start life as a service of one!

### Step 1. Download the Firmware

We release pre-compiled versions of the firmware for the Prusa Mini that can be found on the releases page for this repo. All you need to do is download the `.bbf` file and load it onto the USB stick you use with your machine.

### Step 2. Removing the Appendix on the Prusa Buddy Board

The firmware is not signed so the Prusa bootloader will refuse to accept and flash it onto the device. To flash custom firmware, you need to remove the appendix on the board.

![Breaking the Appendix](https://help.prusa3d.com/wp-content/uploads/2019-12-19-19_52_45-Window-800x224.jpg)

![Appendix Close Up](https://help.prusa3d.com/wp-content/uploads/2019-12-19-19_54_11-Window-800x520.jpg)

Please read this [article](https://help.prusa3d.com/article/flashing-custom-firmware-mini-mini_14) for more information.

### Step 3. Flashing the firmware

Insert the USB stick into the machine and restart or turn on the machine. Click the knob multiple times during the booloader process and it should take you to the following screen.

![Flash Image Screen](https://github.com/jamesgopsill/derusting_book/blob/main/src/assets/flash_firmware.jpg?raw=true)

Click `FLASH` and it will attempt to confirm the signature of the firmware. As our firmware is not signed, you will be prompted with the following screen.

![Warning Screen](https://github.com/jamesgopsill/derusting_book/blob/main/src/assets/warning.jpg?raw=true)

Click `IGNORE` to continue with flashing the firmware onto the device. This will take a few moments and but you should end up at the home screen.

![Home Screen](https://github.com/jamesgopsill/derusting_book/blob/main/src/assets/home_screen.jpg?raw=true)

You should see `DERUSTING` as the title of the screen and the play button, which you would typically use to print, has changed to an `OFFLINE`/`ONLINE` button.

To submit a job to the machine you need to go `http://[IP_ADDRESS_OF_MACHINE]:8080` on your network where you will be presented with:

![Website](https://github.com/jamesgopsill/derusting_book/blob/main/src/assets/website.png?raw=true)

You can submit your job here and the machine will accept it and save it to its USB stick. If you go back to the home screen and set the machine to `ONLINE` by toggling the button then you should see the machine spring to life as it checks the job ledger, notes it is ready to accept jobs and selects from the ledger to print.

> [!NOTE]
> The firmware is set to `dry_print` only at the moment. This restriction can be removed by compiling your own version. We will be adding a toggle for this feature in the future.

### Step 4. Celebrate!

Hooray, you've now entered the world of decentralised manufacturing. No more need for public/private/cloud servers or third-party providers. Simply connect more printers and scale your production. :smile:.

## What is happening behind the scenes.

The machines are communicating on UDP port `9090` where they broadcast their status and share the job ledger and print files when a user uploads one.

- Address Book: Each machine maintains a list of address of the other machines. Machines periodically publish their status over the wire.
- Submission Portal: Users can go to the IP address (:8080) of any of the machines where they can submit their jobs to the system. If a machine is busy it will redirect them to another machine to handle the request.
- Job sharing: Machines share the file amongst one another so it is available on all of their USB sticks for manufacture.
- Job Ledger: The machines pass around a job ledger and each get an opportunity to pick a job from the ledger to manufacture.
- OnReady Function: A machine will only take a job if a user has checked the machine and clicked the button to take it online.

If you want to listen in and see the conversation then please use our derusting udp listener.

> [!TIP]
> You may need to edit your firewall settings on your PC to listen in on the network traffic.

## Where is the trust?

Good spot. We haven't add the trust element to derusting just yet as we're still maturing it and testing it on our research variants of the codebase. Please contact us if you want to learn more on how we're implement trust which is using Web 3.0 technologies - end-to-end encryption, verfiable credentials, smart contracts, zero-knowledge proofs, etc... to enable users and machines to build trust and ensure jobs are manufactured on appropriate machines, there is traceability in the system and privacy is preserved.

## The Book

If you want to learn more about how the `derusting` was created and want to re-create it from scratch yourself the please read the accompanying [book](https://github.com/jamesgopsill/derusting_book) and watch the YouTube series (coming soon).


## Development

Please consult the book if you wish to develop and compile your own version of the firmware.

## Support

Yes please! And there are so many ways you can support us!

- Downloading and using the firmware.
- Raising issues and improvements on the GitHub repo.
- Recommending the project to others.
- Promoting and discussing the project on Social Media (I am on LinkedIn and will be creating a Discord channel too).
- Watch our YouTube videos and join in on livestreams.
- Acknowledging that you use(d) this project to develop your own decentralised manufacturing efforts.
- :star: the project on GitHub.
- [Sponsoring/donating](https://github.com/sponsors/jamesgopsill) to the maintainer and research group enabling us to spend more time in translating our research into industry applicable code for you to adopt, adapt and use to innovate.
- Contacting us and bringing us in to support your decentralised manufacturing efforts. We enjoy consulting and seeing how our projects are inspiring efforts across the world.

## Acknowledgements

## Publications


- [Z. Neu, B. Hicks, and J. Gopsill. “Operating Minimally Intelligent Agent-Based Manufacturing Systems across the Average Demand Interval - Coefficient of Variation (ADI-CV) Demand State Space.” In: *International Journal of Production and Manufacturing Research* (2024).](https://doi.org/10.1080/21693277.2024.2323479)
- [J. Gopsill, M. Goudswaard, L. Giunta, C. Snider, and B. Hicks. “Optimal configurations of minimally intelligent additive manufacturing machines for makerspace production environments”. In: *International Journal of Artificial Intelligence in Engineering Design, Analysis and Manufacture* (2023).](https://doi.org/10.1017/S0890060423000239)


## Other projects
