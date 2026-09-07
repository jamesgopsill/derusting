# derusting - **De**centralised T**rust**ed Manufactur**ing** in **Rust**.

Derusting is a decentralised trusted manufacturing system demonstrator written in Rust and patched into the Prusa Buddy Firmware. The demonstrator turn any of their Additive Manufacturing machines into an decentralised agent that seek each other out on a network to form their own manufacturing collectives. The objective is to showcase how resilient decentralised production systems can be built from the very machines that manufacture the products!

The demonstrator has an accompanying book and YouTube series to get you started with how the build these systems. And please feel free to reach out to me if you want to learn more or would like a hand!

## Getting Started

Ok, so you just want to get stuck in and see what the demonstrator has to offer. To get started with this patch, you will need a Prusa Printer and the following software installed on your machine:

- Git
- Rust
- GCC
- Python
- probe-rs

### Step 1. Removing the Appendix on the Prusa Buddy Board

### Step 2. Download the Repo and Buddy submodule

```bash
git clone https://github.com/jamesgopsill/derusting
```

### Step 3. Build and flash the project.

```bash
bash build_and_flash.sh
```

### What you get...

- Address Book: Each machine maintains a list of address of the other machines. Machines periodically publish their status over the wire.
- Submission Portal: Users can go to the IP address (:8080) of any of the machines where they can submit their jobs to the system. If a machine is busy it will redirect them to another machine to handle the request.
- Job sharing: Machines share the file amongst one another so it is available on all of their USB sticks for manufacture.
- Job Ledger: The machines pass around a job ledger and each get an opportunity to pick a job from the ledger to manufacture.
- OnReady Function: A machine will only take a job if a user has checked the machine and clicked the button to take it online. 

## The Book

If you want to learn more about how the `derusting` was created and want to re-create it from scratch yourself the please read the accompanying [book](https://github.com/jamesgopsill/derusting_book) and watch the YouTube series (coming soon).
