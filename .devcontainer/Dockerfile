FROM rust:1.92-bookworm

ARG TZ
ENV TZ="$TZ"

ARG CLAUDE_CODE_VERSION=latest

# Install system packages and development tools
RUN apt-get update && apt-get install -y --no-install-recommends \
  build-essential \
  pkg-config \
  libssl-dev \
  less \
  git \
  curl \
  wget \
  procps \
  sudo \
  fzf \
  zsh \
  man-db \
  unzip \
  gnupg2 \
  gh \
  iproute2 \
  iptables \
  ipset \
  dnsutils \
  aggregate \
  jq \
  nano \
  vim \
  && apt-get clean && rm -rf /var/lib/apt/lists/*

# Install Node.js (needed for Claude Code)
RUN curl -fsSL https://deb.nodesource.com/setup_20.x | bash - \
  && apt-get install -y --no-install-recommends nodejs \
  && apt-get clean && rm -rf /var/lib/apt/lists/*

# Install solc 0.8.31
ARG SOLC_VERSION=0.8.31
RUN ARCH=$(dpkg --print-architecture) && \
  if [ "$ARCH" = "amd64" ]; then \
    curl -fsSL -o /usr/local/bin/solc https://github.com/ethereum/solidity/releases/download/v${SOLC_VERSION}/solc-static-linux && \
    chmod +x /usr/local/bin/solc; \
  elif [ "$ARCH" = "arm64" ]; then \
    curl -fsSL -o /usr/local/bin/solc https://github.com/argotorg/solidity/releases/download/v${SOLC_VERSION}/solc-static-linux-arm && \
    chmod +x /usr/local/bin/solc; \
  fi

# Install git-delta
ARG GIT_DELTA_VERSION=0.18.2
RUN ARCH=$(dpkg --print-architecture) && \
  wget "https://github.com/dandavison/delta/releases/download/${GIT_DELTA_VERSION}/git-delta_${GIT_DELTA_VERSION}_${ARCH}.deb" && \
  dpkg -i "git-delta_${GIT_DELTA_VERSION}_${ARCH}.deb" && \
  rm "git-delta_${GIT_DELTA_VERSION}_${ARCH}.deb"

# Create non-root user
ARG USERNAME=dev
RUN groupadd --gid 1000 $USERNAME \
  && useradd --uid 1000 --gid $USERNAME --shell /bin/zsh --create-home $USERNAME \
  && echo "$USERNAME ALL=(ALL) NOPASSWD:ALL" > /etc/sudoers.d/$USERNAME \
  && chmod 0440 /etc/sudoers.d/$USERNAME

# Create workspace and config directories
RUN mkdir -p /workspace /home/$USERNAME/.claude \
  && chown -R $USERNAME:$USERNAME /workspace /home/$USERNAME/.claude

# Persist bash history
RUN SNIPPET="export PROMPT_COMMAND='history -a' && export HISTFILE=/commandhistory/.bash_history" \
  && mkdir /commandhistory \
  && touch /commandhistory/.bash_history \
  && chown -R $USERNAME /commandhistory

# Set environment
ENV DEVCONTAINER=true

WORKDIR /workspace

# Switch to non-root user for Rust toolchain setup
USER $USERNAME

# Move rustup to user home (base image installs as root)
ENV RUSTUP_HOME=/home/$USERNAME/.rustup
ENV CARGO_HOME=/home/$USERNAME/.cargo
ENV PATH=/home/$USERNAME/.cargo/bin:$PATH

# Re-install rustup for the dev user
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain 1.92.0

# Install stable components
RUN rustup component add rustfmt clippy rust-src

# Install nightly toolchain (pinned version used by the project)
RUN rustup toolchain install nightly-2026-02-01 --component rust-src --profile minimal

# Install foundry-polkadot (cast + anvil-polkadot for contract deployment/testing)
# Note: foundry-polkadot only provides linux_amd64 binaries
RUN ARCH=$(uname -m) && \
  if [ "$ARCH" = "x86_64" ]; then \
    curl -L https://raw.githubusercontent.com/paritytech/foundry-polkadot/refs/heads/master/foundryup/install | bash \
    && /home/$USERNAME/.foundry/bin/foundryup-polkadot; \
  else \
    echo "Warning: foundry-polkadot not available for $ARCH, skipping"; \
  fi

ENV PATH=/home/$USERNAME/.foundry/bin:$PATH

# Set up zsh with powerlevel10k
ARG ZSH_IN_DOCKER_VERSION=1.2.0
RUN sh -c "$(wget -O- https://github.com/deluan/zsh-in-docker/releases/download/v${ZSH_IN_DOCKER_VERSION}/zsh-in-docker.sh)" -- \
  -p git \
  -p fzf \
  -a "source /usr/share/doc/fzf/examples/key-bindings.zsh" \
  -a "source /usr/share/doc/fzf/examples/completion.zsh" \
  -a "export PROMPT_COMMAND='history -a' && export HISTFILE=/commandhistory/.bash_history" \
  -a 'source "$HOME/.cargo/env"' \
  -x

# Set default shell and editor
ENV SHELL=/bin/zsh
ENV EDITOR=nano
ENV VISUAL=nano

# Install Claude Code
ENV NPM_CONFIG_PREFIX=/home/$USERNAME/.npm-global
ENV PATH=/home/$USERNAME/.npm-global/bin:$PATH
RUN npm install -g @anthropic-ai/claude-code@${CLAUDE_CODE_VERSION}

USER $USERNAME
