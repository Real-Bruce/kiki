# 开源前信息检查与许可整理

日期：2026-11-13。检查基线：`71e7d93`，应用版本 v1.0.9。本次检查、添加 MIT 协议及清理本地旧分支，不代表新的功能或实机验收。

## 结论与需要注意的信息

在检查范围内，未发现已入库的配置密码、API 密钥、访问令牌、私钥或明显的公司／客户工作资料。**存在个人信息和本机环境信息，不能将本报告理解为已经完成脱敏。** 本次按用户要求报告发现项，没有改写提交历史或删除历史证据。

| 位置 | 发现 | 公开影响与处理情况 |
| --- | --- | --- |
| Git 提交和带说明标签的元数据 | 作者／提交者／标签创建者使用同一姓名与个人 Gmail 邮箱；邮箱在此仅记为 `d***@gmail.com`，完整值可用 `git log --all --format=fuller`、`git for-each-ref refs/tags --format='%(tagger)'` 本地查看 | 公开 Git 历史会同时公开这些身份信息。检查到 103 个提交和 21 个带说明标签。保留既有 Git 身份；本次新提交也沿用该身份。仅修改文件或删除旧分支不会消除历史中的邮箱 |
| [STARTUP.md 第 3 行](STARTUP.md)；历史版本的 `docs/P6_HANDOFF.md`、`docs/PROGRESS.md` | 本机盘符和项目目录结构 | 属于低敏感度环境信息，当前文本和历史中仍存在。本次未修改历史报告 |
| 性能与验收报告，例如 [P1](P1.md)、[F11](../long-capture/F11-v1.0.0.md) | 操作系统构建号、CPU、内存、显卡、显示器及远程显示软件等测试环境 | 可识别开发环境，但未发现机器序列号或公司内网地址。属于已有性能证据，保留原记录 |
| 本地 `.idea/workspace.xml` | IDE 账号关联标识、公开 GitHub 用户名、绝对路径、近期编辑和运行配置 | 检查开始时尚未入库。已增加根目录 `/.idea/` 忽略规则，保留用户文件 |
| 本地 `artifacts/` | 配置中的保存目录、Windows 用户目录、构建日志、测试浏览器资料目录 | 已被 Git 忽略，不在源码提交中；直接压缩整个工作目录会包含这些资料。不要将这些本地目录作为开源源码附件 |

`artifacts/settings-scroll/chrome-profile`、`artifacts/settings-scroll-followup/chrome-profile`、`artifacts/settings-slider/chrome-profile` 中存在浏览记录／登录数据库、浏览器偏好和系统加密密钥字段。本次未解密任何字段。首个目录的只读查询得到 1 条 URL 记录、0 条保存登录和 0 条 Cookie；另两个目录部分数据库被锁定或无法打开，不能认定其中没有会话资料。三组 JSON 偏好中未检出邮箱或账号条目。这些是本地测试资料，不是 kiki 的运行配置或依赖。

README 中的 GitHub 地址、协议中的公开账号署名属于有意公开的项目身份。CI 的 `GH_TOKEN: ${{ github.token }}` 是运行时注入引用，没有硬编码令牌值。

## 检查范围与方法

- 通过 `git ls-files` 检查 207 个受控文件，覆盖隐藏配置、Cargo 清单、脚本、源码、双语文档及第三方许可；另查看本地 Git 配置项名称和不含凭据的远端 URL。
- 通过 `git rev-list --objects --all` 与 `git cat-file --batch` 遍历所有本地分支、标签和已缓存远端引用可达的 1,755 个对象，其中 1,041 个为文件对象；包含目前已删除或替换的文件版本。
- 对文本检查常见服务商令牌格式、私钥头、JWT、凭据赋值、带密码 URL、邮箱、个人路径、私网 IPv4、手机号、身份证格式、敏感词及较高熵字符串。复核候选值：受控文件的高熵命中均为 Windows 功能标识或构建路径；本地浏览器资源的命中为语音包名称、扩展测试数据、代码标识、作者邮箱或版本号，未确认用户密钥。
- 对 55 个不同 PNG/JPEG 历史对象制作联系表并实际查看，另查看 3 个不同 ICO 历史对象；内容为产品设计、应用设置和工具条，未发现明显个人／工作内容。检查 PNG 文本／EXIF 块及 JPEG EXIF／XMP／注释段，未发现这些附加元数据。
- 补扫 5,776 个未受控的候选文本文件，包含 `.idea` 和被忽略的日志、配置、测试脚本及浏览器资源。按文件类型筛选、单文件上限 4 MiB，排除 `.git`、`target`、`node_modules` 和本次检查输出目录；二进制内容不按文本扫描。
- 核对当前和历史路径列表，没有发现入库的 `.env`、凭据／私钥文件、应用 `settings.ini` 或 IDE `workspace.xml`。

本次使用本地规则扫描和人工复核，未调用在线密钥验证服务，也没有将仓库内容上传给扫描服务。未安装或运行 Gitleaks／TruffleHog。未检查 Git 不可达对象、reflog 专有历史、远端尚未获取的引用、云端设置或 Release 附件；没有逐张审核被忽略的本地原始截图／录像，也没有解密浏览器存储。结论为本次范围内“未发现”，不是无敏感信息保证。原始候选结果、图片检查副本和清理前引用清单保存在忽略的 `artifacts/open-source-audit/`，不随源码提交。

## MIT 与打包

- 根目录增加 [LICENSE](../../../LICENSE)，采用 [MIT 标准文本](https://opensource.org/license/mit)，署名 `Copyright (c) 2026 Real-Bruce`；署名使用 README 和远端地址已有的公开账号。
- `Cargo.toml` 增加 `license = "MIT"`，双语 README、快速使用说明和许可来源说明同步更新。第三方许可原文保持原样。
- 打包脚本将 `LICENSE` 复制到 ZIP 根目录，发布说明明确旧 ZIP 不会自动补入协议。
- `cargo metadata --no-deps --offline --locked --format-version 1` 成功，返回 `MIT`。
- 在忽略目录的隔离夹具中执行实际打包脚本，复用本机已有 v1.0.9 EXE；构建命令被测试替身跳过，工具链路径使用实际 `rustc --print sysroot` 结果。ZIP 的 17 个条目含项目协议、第三方许可和 Rust 标准库许可；`LICENSE` 与源码逐字一致，SHA-256 校验文件匹配。
- 本次未重新编译应用，未运行 GUI／性能回归，未覆盖 `dist/` 中的既有 ZIP。应用版本、已存在标签和发布产物保持不变。

## 本地分支清理

逐一确认以下 14 个分支均为 `main` 的祖先，且没有工作树检出它们，再使用 `git branch -d` 删除；没有强制删除或丢弃独有提交。

| 已删除分支 | 删除前提交 |
| --- | --- |
| `codex/consolidate-capture-settings` | `c4259ce` |
| `codex/f02-stroke-width` | `c7bd495` |
| `codex/f03-text-size` | `0ae7449` |
| `codex/f04-corner-radius` | `16fe540` |
| `codex/f05-image-formats` | `67f51bb` |
| `codex/f06-reset-capture-settings` | `d28eba8` |
| `codex/f07-capture-tool-visibility` | `89c5322` |
| `codex/f07-toolbar-preview` | `ae4799d` |
| `codex/f08-mosaic` | `834756f` |
| `codex/f09-f10` | `1e295de` |
| `codex/f11-long-capture` | `6c66508` |
| `codex/f12-toolbar` | `9ffe32d` |
| `codex/feature-design-baseline` | `e1be331` |
| `codex/fix-long-capture` | `0c7d28a` |

保留 `main` 和当前 `codex/docs-module-guide`：后者包含尚未合入 `main` 的文档及本次开源准备工作。对比清理前后引用，所有标签及远端跟踪引用保持不变；没有推送、远端分支删除或 Release 发布。

本地提交主题：`chore: prepare MIT licensing and record open-source audit`。提交前检查 `git diff --check`、明确暂存的 diff、文档链接、Cargo 许可元数据、隔离打包结果及忽略规则。
