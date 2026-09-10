# derusting - **De**centralised T**rust**ed Manufactur**ing** in **Rust**.

Derusting is a **de**centralised t**rust**ed manufactur**ing** system demonstrator written in **Rust** and patched into the Prusa Buddy Firmware. The demonstrator turns any of their Additive Manufacturing machines into decentralised agents that seek each other out on a network to form their own manufacturing collectives. The objective is to showcase and alternative to centralised systems which have challenges concerning single-point of failures, centralised control and governance and ownership of Intellectual Property. Decentralised systems can mitigate much of these concerns by empowering the very machines to manage and negotiate the jobs they manufacture in a trusted secure, resilient and responsive environment!

<p align="center">
  <img src="https://github.com/jamesgopsill/derusting_book/blob/main/src/assets/lab.png?raw=true" height="400" alt="Lab">
</p>

The demonstrator has an accompanying book and YouTube series to get you started with the principles of building decentralised systems. Please feel free to reach out if you want to learn more or would like a hand!

## Getting Started

You can jump right in by using a pre-compiled firmware for the Prusa Mini or adapt and build your own using the development steps further below. Here, we show you how to get started with the pre-compiled version. And don't worry about needing lots of printers. A de-centralised service can start life as a service of one!

### Step 1. Download the Firmware

We release pre-compiled versions of the firmware for the Prusa Mini that can be found on the releases page for this repo. All you need to do is download the `.bbf` file and load it onto the USB stick you use with your machine.

### Step 2. Removing the Appendix on the Prusa Buddy Board

The firmware is not signed so the Prusa bootloader will refuse to accept and flash it onto the device. To flash custom firmware, you need to remove the appendix on the board.

<p align="center">
  <img src="https://help.prusa3d.com/wp-content/uploads/2019-12-19-19_52_45-Window-800x224.jpg" height="400" alt="Breaking the Appendix">
</p>

<p align="center">
  <img src="https://help.prusa3d.com/wp-content/uploads/2019-12-19-19_54_11-Window-800x520.jpg" height="400" alt="Appendix Close Up">
</p>

Please read this [article](https://help.prusa3d.com/article/flashing-custom-firmware-mini-mini_14) for more information.

### Step 3. Flashing the firmware

Insert the USB stick into the machine and restart or turn on the machine. Click the knob multiple times during the bootloader process and it should take you to the following screen.

<p align="center">
  <img src="https://github.com/jamesgopsill/derusting_book/blob/main/src/assets/flash_device.png?raw=true" height="400" alt="Flashing Custom Firmware">
</p>

Click `FLASH` and it will attempt to confirm the signature of the firmware. Our firmware is not signed by Prusa so you will need to click `IGNORE` to continue with flashing the firmware onto the device. This will take a few moments and but you should end up at the home screen. You should see `DERUSTING` as the title of the screen and the play button, which you would typically use to print, has changed to an `OFFLINE`/`ONLINE` button.

To submit a job to the machine you need to go `http://[IP_ADDRESS_OF_MACHINE]:8080` on your network where you will be presented with:

<p align="center">
  <img src="https://github.com/jamesgopsill/derusting_book/blob/main/src/assets/website.png?raw=true" height="400" alt="Website">
</p>

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

## Shout-outs

- Youtubers showing me how to Rust.
  - [Jon Gjengset](https://www.youtube.com/@jonhoo)
  - [The Rusty Bits](https://www.youtube.com/@therustybits)
  - [Floodplain](https://www.youtube.com/@floodplainnl)
- UKRI who have funded my [Brokering Additive Manufacturing](https://gtr.ukri.org/projects?ref=EP%2FV05113X%2F1) grant and [Innovation Launchpad Network+ Researcher-in-Residence and Booster Award Schemes](https://innovationlaunchpad.group.shef.ac.uk/).
- Prusa and their opensource/hardware software repositories that enable us to tinker with the awesome machines and inspire future innovations.
	- [Prusa Buddy Firmware](https://github.com/prusa3d/Prusa-Firmware-Buddy)
	- [Prusa Buddy Board Documentation](https://github.com/prusa3d/Buddy-board-MINI-PCB)
	- [Pusa BOM Documentation](https://github.com/prusa3d/Original-Prusa-MINI)
- The Design and Manufacturing Futures (DMF) lab where I am based and working with my awesome research group of academics, researchers, tech team, PhDs, postgrads and undergrads, and the School, Faculty and University.

## Publications

- [J. Gopsill, O. Schiffmann, C. Ranscombe, and M. Goudswaard. “Secure by design: exploring a minimal Web3.0 trust network to provide de-centralised secure, private, and provenance preserving design and manufacture workflow”. In: 19th International DESIGN conference. 2026.](https://doi.org/10.1017/pds.2026.10550)
- [J. Gopsill and P. Walker-Davies. “Using Web3.0 to build trust in agent-based additive manufacturing systems”. In: 58th CIRP Conference on Manufacturing Systems. 2025.](https://doi.org/10.1016/j.procir.2025.02.183)
- [Z. Neu, B. Hicks, and J. Gopsill. “Operating Minimally Intelligent Agent-Based Manufacturing Systems across the Average Demand Interval - Coefficient of Variation (ADI-CV) Demand State Space.” In: *International Journal of Production and Manufacturing Research* (2024).](https://doi.org/10.1080/21693277.2024.2323479)
- [J. Gopsill. “Distributed Additive Manufacturing: A Social Change in Manufacturing”. In: 31st International Conference on Transdisciplinary Engineering 2024. 2024.](https://doi.org/10.3233/ATDE240850)
- [J. Gopsill, C. Cox, and B. Hicks. “Global Local (Glocal) Supply Chains for Green Economies: An assessment of Greenhouse Gas Emissions”. In: *International Conference on Manufacturing Research*. 2024.](https://doi.org/10.1051/matecconf/202440105001)
- [J. Gopsill, M. Goudswaard, L. Giunta, C. Snider, and B. Hicks. “Optimal configurations of minimally intelligent additive manufacturing machines for makerspace production environments”. In: *International Journal of Artificial Intelligence in Engineering Design, Analysis and Manufacture* (2023).](https://doi.org/10.1017/S0890060423000239)
- [O. Peckham, M. Goudswaard, C. Snider, and J. Gopsill. “What to Share? A Preliminary Investigation into the Impact of Information Sharing on Distributed Decentralised Agent-Based Additive Manufacturing Networks”. In: *Advances in Production Management Systems. Production Management Systems for Responsible Manufacturing, Service, and Logistics Futures*. 2023.](https://doi.org/10.1007/978-3-031-43666-6_36)
- [L. Giunta, B. Hicks, C. Snider and J. Gopsill. “A Living Lab Platform for Testing Additive Manufacturing Agent-Based Manufacturing Strategies”. *Proceedings of the 33rd CIRP Design Conference*. 2023.](https://doi.org/10.1016/j.procir.2023.03.118)
- [M. Goudswaard, C. Snider, M. Obi, L. Giunta, K. Ramli, J. Johns, B. Hicks and J. Gopsill. “Required parameters for modelling heterogeneous geographically dispersed manufacturing systems”. *Procedia CIRP*. 2022.](https://doi.org/10.1016/j.procir.2022.05.189)
- [L. Giunta, M. Obi, M. Goudswaard, B. Hicks and J. Gopsill. “Comparison of Three Agent-Based Architectures for Distributed Additive Manufacturing”. *Procedia CIRP*. 2022.](https://doi.org/10.1016/j.procir.2022.05.123)
- [M. Obi, C. Snider, L. Giunta, M. Goudswaard and J. Gopsill. “Coping with diverse product demand through agent-led type transitions”. *Proceedings of the 16th International KES Conference on Agent & Multi-Agent Systems: Technologies & Applications*. 2022.](https://link.springer.com/book/9789811933585) [Video](https://youtu.be/5RDMJ4J048U)
- [J. Gopsill, M. Obi, L. Giunta and M. Goudswaard. “Queueless: Agent-Based Manufacturing for Workshop Production”. *Proceedings of the 16th International KES Conference on Agent & Multi-Agent Systems: Technologies & Applications*. 2022.](https://link.springer.com/book/9789811933585) [Video](https://youtu.be/mWcQRkU0pBs)
- [M. Goudswaard, J. Gopsill, A. Ma, A. Nassehi, and B. Hicks. “Responding to rapidly changing product demand through a coordinated additive manufacturing production system: a COVID-19 case study”. In: *Proceedings of the Manufacturing Engineering Society International Conference*. 2021.](https://iopscience.iop.org/article/10.1088/1757-899X/1193/1/012119/meta)
- [J. Gopsill**, M. Goudswaard, C. Snider, J. Johns, and B. Hicks. “Achieving responsive and sustainable manufacturing through a brokered agent-based production paradigm”. In: *Proceedings of Sustainable Design and Manufacturing*. 2021.](https://doi.org/10.1007/978-981-16-6128-0_3) [Video](https://youtu.be/BevYAGvTnsg)


## Other projects

- https://crates.io/crates/meatpack - A Rust implementation of a gcode encoding algorithm that can increase the data density by a factor of two.
- https://crates.io/crates/egcode - An encrypted gcode standard for end-to-end encryption of design IP.
- https://jamesgopsill.github.io/egcode/ - A demonstrator website for encrypting gcode.
- https://crates.io/crates/binarygcode - A Rust implementation of the binary gcode standard.
- https://crates.io/crates/buddy_client - A client for the Buddy API.
- https://github.com/jamesgopsill/embassy-buddy - A Board Support Crate for the embassy embedded runtime and Prusa Buddy Board.
