# Vibe Voice Lite

<p align="center"><img src="assets/app-icon.png" width="112" alt="Vibe Voice Lite"></p>

Windows 上的按住说话工具：语音转写、AI 整理开发需求，以及逐轮提问的需求访谈。

[English](README.en.md) · [识别模型](docs/MODELS.zh-CN.md) · [本地语音播报](docs/TTS.md)

## 使用方式

启动 `vibe-voice-lite.exe` 后，屏幕底部出现小圆点。先点击目标输入框，按住 **右 Ctrl** 说话，松开后识别。托盘右键可打开设置、选择模式、切换自动粘贴、退出。

| 模式 | 行为 |
| --- | --- |
| 普通口述 | 直接转写；可选择使用 AI 整理 |
| 代码整理 | 使用 AI 整理开发需求，保留原意、改口、约束及明确待定项 |
| 需求访谈 | 主动逐轮提问，每次一个重点，保留历史回答；结束后输出已确定和待定事项 |

普通反馈与测试语句不强制转成需求，不因缺少实现细节而追问。真正影响原意的歧义才进入确认流程。AI 仍可能误解口述，请核对最终结果。

### 需求访谈与控制口令

1. 托盘选择 **需求访谈（逐步提问）**。
2. 按住右 Ctrl 说出目标，例如“我想做个待办工具，帮我理清需求”。
3. 听完一个问题后，继续按住右 Ctrl 回答。可以说“暂时不定，先跳过”。
4. 单独说 **“先整理到这里”**，结束并输出“已确定”“待定”两部分。

- **取消这次整理 / 先不处理**：在等待补充时取消，不生成最终总结。
- **Esc**：在等待回答时取消。录音或处理期间并非全局取消键。
- **按住说话键**：打断当前 TTS，开始回答。
- **切换模式 / 退出程序**：取消本次对话。访谈内容仅保存在当前进程内。
- 单独说“停止”目前不是固定控制口令。

启用自动粘贴时，只有最终结果会写入剪贴板并发送 Ctrl+V。若处理期间切换窗口或仍按着修饰键，会跳过自动发送；已复制的结果可手动 Ctrl+V。输入框是否接受快捷键取决于目标软件及权限，界面不会把按键发送等同于确认粘贴成功。

## 安装与模型

前往 [Windows 构建列表](https://github.com/UtenKekkoDev/vibe-voice-lite/actions/workflows/ci.yml?query=branch%3Amain)，打开最近一次成功的 main 构建，在 **Artifacts** 下载 **windows-ci-bundle**（需登录 GitHub）。解开外层压缩包，再解压里面的 `vibe-voice-lite-windows-x64.zip`，运行 `vibe-voice-lite.exe`。正式版会列在 [Releases](https://github.com/UtenKekkoDev/vibe-voice-lite/releases)。

需要 Windows 10/11 x64、麦克风、识别模型，以及 [Microsoft Visual C++ x64 运行库](https://aka.ms/vs/17/release/vc_redist.x64.exe)。当前构建没有数字签名，**不包含模型权重、API Key、Python 或 CUDA 环境**。

- 本地识别：下载官方 sherpa-onnx SenseVoice INT8 模型，在设置中选择模型目录并检查。见[模型说明](docs/MODELS.zh-CN.md)。也可放在 exe 旁的 `models/` 下。
- 在线识别：可选 Qwen-ASR Realtime，将音频发送到配置的服务。
- AI 整理 / 访谈：配置 DeepSeek、OpenAI、Ollama 或兼容 Chat Completions 的接口、模型及密钥。模型名称应使用账号实际可用的值。
- 本地 TTS：可选 Qwen3-TTS 1.7B CustomVoice / Serena。当前采用轻柔亲切、语速稍慢的风格；仅播报确认和访谈问题，不朗读完成回执或最终正文。需要 NVIDIA CUDA GPU，配置见 [TTS setup](docs/TTS.md)。

Qwen 语音后台预热后驻留；重启语音或退出程序会释放它。固定风格和随机种子可减少随机变化，但不同句子仍有自然的重音、停顿变化。

## 隐私与配置

- 设置存放于 `%LOCALAPPDATA%/VibeVoiceLite/settings.json`。
- AI Key 使用当前 Windows 用户的 DPAPI 加密，绑定目标接口；清空字段会删除，切换服务或接口会清空旧密钥。Qwen-ASR Key 仅保留在本次会话。
- 原始录音、转写和问答不写入设置；生命周期日志只记状态与性能元数据。主动开启 `VIBE_LITE_DEBUG` 的调试日志可能含文本，不要公开上传。
- **AI 整理和需求访谈会把转写、术语表、历轮问答发送到所选服务。** 本地 SenseVoice 和本地 TTS 本身不上传内容。详见 [SECURITY.md](SECURITY.md)。
- `tts/runtime.json` 是本机私有路径配置，不提交到 Git；仓库只提供 `runtime.example.json`。

## 从源码构建

需要 Rust stable（支持 edition 2024 / let chains）、Visual Studio C++ Build Tools 和 Windows SDK。在 **Developer PowerShell for VS** 中执行，确保 `rc.exe` 可用：

```powershell
cargo build --locked --release --bin vibe-voice-lite
```

首次构建需联网下载 crates 和 ONNX Runtime。项目内包含官方 pollster 0.4.0 源码，以及修正了新版 Cargo DLL 复制路径的 ort-sys 构建兼容补丁；不依赖开发者电脑上的路径。

```powershell
cargo fmt --all --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets
python -m compileall -q tts scripts
python scripts/audit_release.py
./scripts/package.ps1
```

打包脚本只复制程序、运行 DLL、TTS 源码、配置示例和文档，输出 ZIP 与 SHA-256；不会打包本机模型、配置或录音。

`./run-native.ps1` 从源码运行并设置 TTS 目录。图标源文件是 `assets/app-icon.svg`，运行 `python scripts/generate_icons.py` 可重新生成 PNG、ICO 和 RGBA；不需要图形库。exe、窗口和托盘共用同一个图案。

普通测试不需要网络服务账号或模型；真实模型/设备/API 测试默认忽略，运行方式见 [CONTRIBUTING.md](CONTRIBUTING.md)。

## 限制与来源

当前为早期版本，未签名。自动粘贴不支持跨越 Windows 权限边界；换电脑需要重新配置模型和可选 TTS 环境。AI 是否追问仍取决于模型判断，不能保证每次都正确。

基于 [UtenKekkoDev/vibe-coding-voice](https://github.com/UtenKekkoDev/vibe-coding-voice) 演进，保留原项目的语音识别基础并重写界面、投递、AI 与 TTS 流程。

MIT，见 [LICENSE](LICENSE) 与 [第三方说明](THIRD_PARTY_NOTICES.md)。
