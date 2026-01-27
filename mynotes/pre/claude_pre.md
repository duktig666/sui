项目初始优化CLAUDE.md和skills：
1. CLAUDE.md 翻译为中文 并添加规则：代码使用英文，文档和回答、注释等使用中文。
2. 当前项目主要是研究性质，研究 Sui 和 DEX 的实现，以及他们的一些机制是否可以进行结合。主要是对项目分析、梳理，以及调研和思考如何借助 sui 实现一套媲美 hyperliquid 的 DEX。 技术栈主要使用 rust。 DEX 中理想的实现架构可以参看 notes 下的一些文档。 notes 下是团队梳理调研的优质文档，mynotes 是我将要思考和梳理出的文档。 基于此帮我完善 CLAUDE.md。
3. .claude/agents 下的几个文件是我定义的几个角色，用来方便进行 ai 交互。analyst 自身业务分析师，专注于 dex、高频交易系统、区块链和 sui 的调研和分析，architect 主要根据 analyst 的分析结果进行方案和架构的分析及设计，engineer 主要根据 analyst 的分析和 rchitect 的设计进行项目的开发和实施，主要使用 rust 借助 sui 来开发 DEX。 帮我完善.claude/agents 的几个角色使他们更智能。
4. mynotes/dex 下我放置了一些之前生成和 dex 相关的 prd，数据结构等设计，可以用来优化 CLAUDE.md、skills 或是平常的分析。
5. 结合上述内容，在.claude/skills下生成一些符合claude code标准的skills，用以优化后续和ai的交互体验。