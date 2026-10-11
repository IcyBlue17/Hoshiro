# Linux 编译环境：Ubuntu 22.04（glibc 2.35，产物能在 22.04 及更新的发行版上运行）+ Tauri 系统依赖 + Rust + Bun。
# 由 .github/workflows/linux-image.yml 构建并推送到 ghcr.io/<owner>/hoshiro-linux-build:22.04，每周重建一次拿到安全更新。
FROM ubuntu:22.04

ARG DEBIAN_FRONTEND=noninteractive
# eatmydata 把 apt / dpkg 的 fsync 变成空操作：构建机硬盘的同步写入极慢（每次 fsync 约 0.5 秒），
# 镜像层本来就是一次性写入，不需要逐个文件落盘。
RUN apt-get update \
 && apt-get install -y --no-install-recommends eatmydata \
 && eatmydata apt-get install -y --no-install-recommends \
      build-essential curl wget file git unzip ca-certificates pkg-config xdg-utils \
      libssl-dev libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev libasound2-dev \
 && rm -rf /var/lib/apt/lists/*

# APPIMAGE_EXTRACT_AND_RUN：容器里没有 FUSE，打 AppImage 时让 linuxdeploy 自解压运行
ENV RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    BUN_INSTALL=/usr/local \
    PATH=/usr/local/cargo/bin:$PATH \
    APPIMAGE_EXTRACT_AND_RUN=1

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path --profile minimal --default-toolchain stable \
 && curl -fsSL https://bun.sh/install | bash \
 && rustc --version && cargo --version && bun --version
