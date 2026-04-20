export default {
  slug: 'pikpaktui',
  install: {
    linux:   { name: 'Linux',   cmd: 'curl -fsSL https://app.snaix.homes/pikpaktui/install | bash', note: 'debian, ubuntu, arch, alpine, fedora · musl static · x86_64 + arm64' },
    macos:   { name: 'macOS',   cmd: 'brew install Bengerthelorf/tap/pikpaktui',                     note: 'homebrew; universal binary — arm64 + x86_64' },
    cargo:   { name: 'Cargo',   cmd: 'cargo install pikpaktui',                                      note: 'builds from crates.io · rust 1.78+' },
    source:  { name: 'source',  cmd: 'git clone https://github.com/Bengerthelorf/pikpaktui.git && cd pikpaktui && cargo build --release', note: 'binary at ./target/release/pikpaktui' },
  },
  sections: [
    {
      label: 'guide',
      items: ['getting-started', 'tui', 'configuration', 'shell-completions'],
    },
    {
      label: 'cli',
      items: ['cli/index', 'cli/commands'],
    },
  ],
  linkRewrites: {
    '/guide/': '/docs/',
    '/cli/':   '/docs/cli/',
  },
};
