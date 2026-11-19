# Dockerfile pour build Iced_Yogurt avec toutes les dépendances système
# Utilisé par GitHub Actions pour éviter les problèmes de dépendances

FROM ubuntu:22.04

# Variables d'environnement
ENV DEBIAN_FRONTEND=noninteractive
ENV RUSTUP_HOME=/usr/local/rustup
ENV CARGO_HOME=/usr/local/cargo
ENV PATH="/usr/local/cargo/bin:/usr/local/rustup/bin:$PATH"

# Installer les dépendances système nécessaires
RUN apt-get update && \
    apt-get install -y --no-install-recommends \
    # Outils de base
    ca-certificates \
    curl \
    git \
    gcc \
    g++ \
    make \
    cmake \
    pkg-config \
    
    # Dépendances pour whisper-rs-sys (whisper.cpp)
    clang \
    libclang-dev \
    libclang1 \
    llvm \
    
    # Dépendances pour cpal (audio)
    libasound2-dev \
    libudev-dev \
    libpulse-dev \
    libjack-dev \
    
    # Dépendances pour SQLite
    libsqlite3-dev \
    
    # Nettoyage
    && rm -rf /var/lib/apt/lists/*

# Installer Rust stable
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal

# Configurer LIBCLANG_PATH
RUN echo "export LIBCLANG_PATH=/usr/lib/llvm-14/lib" >> /etc/profile.d/libclang.sh && \
    echo "export PKG_CONFIG_PATH=/usr/lib/x86_64-linux-gnu/pkgconfig:/usr/share/pkgconfig" >> /etc/profile.d/pkgconfig.sh

# Créer un utilisateur non-root pour la sécurité
RUN useradd -m -u 1000 builder

# Configurer le répertoire de travail
WORKDIR /workspace

# Copier le code source (sera fait par GitHub Actions)
# Le code sera monté via actions/checkout

# Build le projet
# RUN chown -R builder:builder /workspace && \
#     su builder -c "cargo build --workspace"

# Commande par défaut
CMD ["bash"]
