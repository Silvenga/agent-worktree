# Scripts

## build-npm.sh

构建 npm 分发包，编译 Rust 二进制并复制到对应平台包。

```bash
# 构建当前平台
./scripts/build-npm.sh current

# 构建所有平台 (需要 cross)
./scripts/build-npm.sh all

# 构建指定平台
./scripts/build-npm.sh darwin-arm64
./scripts/build-npm.sh darwin-x64
./scripts/build-npm.sh linux-x64
./scripts/build-npm.sh win32-x64
```

跨平台编译需要先安装 [cross](https://github.com/cross-rs/cross) 和 Docker:

```bash
cargo install cross
```

注意: cross 不支持 macOS 目标。在 macOS 上构建时:
- darwin-arm64/darwin-x64 使用 `cargo build` (需要 `rustup target add`)
- linux-x64/win32-x64 使用 `cross` (需要 Docker)

## publish-npm.sh

发布 npm 包到 registry。

```bash
# 使用 Cargo.toml 中的版本
./scripts/publish-npm.sh

# 指定版本
./scripts/publish-npm.sh 0.1.0

# dry-run 模式 (不实际发布)
./scripts/publish-npm.sh 0.1.0 true
```

发布前需要:

1. 先跑 `build-npm.sh`，四个平台二进制全部就位且为当前版本
2. `npm login` 登录 npm 账号

发布闸门：逐平台校验二进制。缺失、或 `strings` 抽出的内嵌版本不含待发版本，一律 error 退出。
禁绕过 —— 主包 pin 精确版本，跳过任一平台即发出该平台上装不起来的版本；版本不符则发出「新版号 + 旧代码」的假版本。

发布顺序: 平台包 → 主包 (因为主包依赖平台包)
