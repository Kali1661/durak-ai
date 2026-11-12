# Installing and running Durak AI

**English** · [Slovensky](INSTALL.sk.md) · [Back to the README](README.md)

This guide assumes no programming experience. It is written for **Windows**;
macOS and Linux are at the end. Setting up takes 15-30 minutes the first time,
mostly waiting for downloads. After that, starting the app takes one command.

## Before you start: how to type commands

You will type commands into **Command Prompt**:

1. Press the **Windows key**, type `cmd`, press **Enter**. A black window opens.
2. Type (or paste) **one command at a time**, press **Enter**, and wait until it
   finishes - the blinking cursor comes back after a line like
   `C:\Users\YourName>`. Then type the next one.
3. Read what it printed before going on. If you paste several lines at once,
   Command Prompt runs them one after another even when an earlier one failed,
   and the error you need gets buried.

> **Most important rule:** after you install a program in steps 1-3, **close the
> Command Prompt window and open a new one.** A window only knows about programs
> that were installed *before* it was opened. Almost every "is not recognized"
> error comes from skipping this.

## Step 1 - Install the tools

You need four programs. Run these three lines one by one; each downloads and
installs a program (Windows may ask for permission - click **Yes**):

```bat
winget install --id Git.Git -e
winget install --id OpenJS.NodeJS.LTS -e
winget install --id Rustlang.Rustup -e
```

If `winget` itself "is not recognized", update **App Installer** from the
Microsoft Store, or download the installers by hand:
[Git](https://git-scm.com/download/win),
[Node.js (LTS)](https://nodejs.org/),
[Rust](https://rustup.rs/).

**Now close Command Prompt and open a new one.**

Check that everything is there - each line should print a version number:

```bat
git --version
node --version
cargo --version
```

## Step 2 - Add WebAssembly support to Rust

Run these two lines (the second one takes **5-10 minutes**; it is normal for it
to print many "Compiling ..." lines):

```bat
rustup target add wasm32-unknown-unknown
cargo install wasm-pack
```

**Close Command Prompt and open a new one again**, then check:

```bat
wasm-pack --version
```

## Step 3 - Download the project

Choose a place for it, for example your Documents folder, and download it there:

```bat
cd %USERPROFILE%\Documents
git clone https://github.com/Kali1661/durak-ai.git
```

This creates a folder `Documents\durak-ai`.

*Without Git:* on the [project page](https://github.com/Kali1661/durak-ai) click
the green **Code** button, then **Download ZIP**, and unzip it into Documents.
The folder will be called `durak-ai-main` instead - use that name in the next step.

## Step 4 - Install the app and start it

Go into the app's folder. The commands only work from **this exact folder**:

```bat
cd %USERPROFILE%\Documents\durak-ai\durak-online\web
```

The line before the cursor should now end with `durak-online\web>`. Then:

```bat
npm install
npm run dev
```

`npm install` downloads what the app needs (about a minute, only needed once).
`npm run dev` builds the game engine the first time (a minute or two) and then
prints something like:

```
  VITE ready in 900 ms
  ➜  Local:   http://localhost:5173/
```

Open **http://localhost:5173** in your web browser. That's the app.

**Leave the Command Prompt window open** while you use the app - closing it
stops the app. To stop it on purpose, click into the window and press
**Ctrl + C**.

## Next time

Open Command Prompt and run just these two lines:

```bat
cd %USERPROFILE%\Documents\durak-ai\durak-online\web
npm run dev
```

## Getting the latest version

```bat
cd %USERPROFILE%\Documents\durak-ai
git pull
cd durak-online\web
npm install
npm run dev
```

`npm run dev` notices when the game engine changed and rebuilds it by itself.

## When something goes wrong

| What you see | What it means and what to do |
|---|---|
| `'git'`, `'node'`, `'npm'`, `'cargo'` or `'wasm-pack'` **is not recognized as an internal or external command** | The program is not installed, or this window was opened before it was installed. Close Command Prompt, open a new one, try again. If it still happens, redo that program's step. |
| `The system cannot find the path specified.` after a `cd` command | The folder is not where the command expects. Type `dir` to see what is in the current folder, or check where you downloaded the project (step 3). If you used the ZIP, the folder is `durak-ai-main`. |
| `npm ERR! enoent ... package.json` | You are in the wrong folder. Run the `cd ...\durak-online\web` line from step 4 first. |
| `Could not build the game engine` | wasm-pack is missing: do step 2, then open a new window. |
| `linker 'link.exe' not found` (during step 2) | Rust on Windows needs Microsoft's C++ build tools. Run the line below, wait for it to finish (it is a large download), open a new window and repeat step 2. |
| `Port 5173 is in use, trying another one...` | The app is probably already running in another window. That is fine - open the address the new window prints (for example `http://localhost:5174`). |
| In PowerShell: `running scripts is disabled on this system` | Use Command Prompt (`cmd`) instead of PowerShell, as this guide does. |
| The page is blank or shows an old version | Press **Ctrl + F5** in the browser to reload it fully. |

C++ build tools (only if you saw the `link.exe` error):

```bat
winget install --id Microsoft.VisualStudio.2022.BuildTools -e --override "--passive --wait --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
```

If you are stuck, [open an issue](https://github.com/Kali1661/durak-ai/issues)
and paste the **whole** text from the Command Prompt window.

## macOS and Linux

Install [Git](https://git-scm.com/), [Node.js 18 or newer](https://nodejs.org/)
and [Rust](https://rustup.rs/) (on macOS: `xcode-select --install` first, then
Node and Rust from their websites or Homebrew). Then in a terminal:

```bash
rustup target add wasm32-unknown-unknown
cargo install wasm-pack
git clone https://github.com/Kali1661/durak-ai.git
cd durak-ai/durak-online/web
npm install
npm run dev
```

and open <http://localhost:5173>.

## For developers

| Command (in `durak-online/web`) | What it does |
|---|---|
| `npm run dev` | Start the development server (rebuilds the engine if the Rust code changed) |
| `npm run build` | Rebuild the engine and write a static site to `dist/`, which can be hosted anywhere |
| `npm run wasm` | Rebuild only the engine |
| `npm test` | Browser-side tests, including end-to-end tests against the real engine |

Engine tests: `cd durak-online` then `cargo test -p durak-core --release`.
