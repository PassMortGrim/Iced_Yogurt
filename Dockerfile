# Dockerfile pour builder Iced_Yogurt avec toutes les dépendances
# Utilisation : docker build -t iced-yogurt . && docker run -it iced-yogurt

FROM ubuntu:22.04

# Variables d'environnement
ENV DEBIAN_FRONTEND=noninteractive
ENV RUSTUP_HOME=/usr/local/rustup
ENV CARGO_HOME=/usr/local/cargo
ENV PATH="/usr/local/cargo/bin:/usr/local/rustup/bin:$PATH"

# Installer les dépendances système
RUN apt-get update && \
    apt-get install -y --no-install-recommends \
    ca-certificates \
    curl \
    git \
    gcc \
    g++ \
    make \
    cmake \
    pkg-config \
    clang \
    libclang-dev \
    libclang1 \
    llvm \
    libasound2-dev \
    libudev-dev \
    libsqlite3-dev \
    && rm -rf /var/lib/apt/lists/*

# Installer Rust
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile default

# Configurer les variables d'environnement pour whisper-rs
ENV LIBCLANG_PATH=/usr/lib/llvm-14/lib
ENV PKG_CONFIG_PATH=/usr/lib/x86_64-linux-gnu/pkgconfig:/usr/share/pkgconfig

# Créer un utilisateur non-root
RUN useradd -m -u 1000 builder
WORKDIR /workspace

# Copier le code et builder
COPY . /workspace
RUN chown -R builder:builder /workspace
USER builder

# Builder le projet
RUN cargo build --workspace --no-default-features

# Commande par défaut
CMD ["cargo", "run", "--bin", "iced-yogurt", "--", "--help"]
