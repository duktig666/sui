// DAG Visualizer - 将 Mysticeti DAG 导出为 DOT 格式用于可视化

use clap::Parser;
use consensus_study::dag_builder::{DagBuilder, TestBlock};
use consensus_study::BlockRef;

#[derive(Parser)]
#[command(name = "dag-visualizer")]
#[command(about = "Visualize Mysticeti DAG in DOT format", long_about = None)]
struct Cli {
    /// Number of authorities/validators
    #[arg(short, long, default_value_t = 4)]
    committee_size: usize,

    /// Number of rounds to generate
    #[arg(short, long, default_value_t = 5)]
    rounds: u32,

    /// Output file path (defaults to stdout)
    #[arg(short, long)]
    output: Option<String>,

    /// Wave length for highlighting
    #[arg(short, long, default_value_t = 3)]
    wave_length: u32,
}

fn main() {
    let cli = Cli::parse();

    // 构建 DAG
    let mut builder = DagBuilder::new(cli.committee_size);
    for round in 1..=cli.rounds {
        builder.add_round(round);
    }

    // 生成 DOT 格式
    let dot = generate_dot(&builder, cli.wave_length);

    // 输出
    match cli.output {
        Some(path) => {
            std::fs::write(&path, dot).expect("Failed to write output file");
            println!("DAG visualization written to: {}", path);
            println!("\nTo generate PNG:");
            println!("  dot -Tpng {} -o dag.png", path);
            println!("\nTo view interactively:");
            println!("  xdot {}", path);
        }
        None => {
            println!("{}", dot);
        }
    }

    // 打印统计
    let stats = builder.stats();
    eprintln!("\nDAG Statistics:");
    eprintln!("  Total blocks: {}", stats.total_blocks);
    eprintln!("  Rounds: {}", stats.rounds);
    eprintln!("  Max round: {}", stats.max_round);
    eprintln!("  Committee size: {}", stats.committee_size);
    eprintln!("  Avg ancestors per block: {:.2}", stats.avg_ancestors);
}

fn generate_dot(builder: &DagBuilder, wave_length: u32) -> String {
    let mut dot = String::new();

    // DOT header
    dot.push_str("digraph MysticetiDAG {\n");
    dot.push_str("  rankdir=BT;\n"); // Bottom to Top (低轮次在下)
    dot.push_str("  node [shape=box, style=filled];\n");
    dot.push_str("  \n");

    // 为不同 wave 定义颜色
    let wave_colors = vec![
        "#FFE6E6", // Wave 0 - 浅红
        "#E6F3FF", // Wave 1 - 浅蓝
        "#E6FFE6", // Wave 2 - 浅绿
        "#FFF3E6", // Wave 3 - 浅橙
        "#F3E6FF", // Wave 4 - 浅紫
    ];

    let author_shapes = vec!["box", "ellipse", "diamond", "hexagon"];

    // 添加所有节点
    for block in builder.all_blocks() {
        let block_ref = block.reference();
        let wave = (block_ref.round / wave_length) as usize;
        let color = wave_colors.get(wave % wave_colors.len()).unwrap_or(&"#FFFFFF");
        let shape = author_shapes.get(block_ref.author.value() as usize % author_shapes.len())
            .unwrap_or(&"box");

        let is_leader_round = block_ref.round % wave_length == 0;
        let label = if is_leader_round {
            format!(
                "R{} A{}\\n[LEADER]",
                block_ref.round, block_ref.author.value()
            )
        } else {
            format!("R{} A{}", block_ref.round, block_ref.author.value())
        };

        let style = if is_leader_round {
            "filled,bold"
        } else {
            "filled"
        };

        dot.push_str(&format!(
            "  \"{}\" [label=\"{}\", fillcolor=\"{}\", shape={}, style=\"{}\"];\n",
            block_id(&block_ref),
            label,
            color,
            shape,
            style
        ));
    }

    dot.push_str("\n");

    // 添加边（祖先引用）
    for block in builder.all_blocks() {
        let block_ref = block.reference();
        for ancestor in &block.ancestors {
            // 区分强链接和弱链接
            // 简化版：round - 1 是强链接，其他是弱链接
            let edge_style = if ancestor.round == block_ref.round - 1 {
                "solid"
            } else {
                "dashed"
            };

            dot.push_str(&format!(
                "  \"{}\" -> \"{}\" [style={}];\n",
                block_id(&block_ref),
                block_id(ancestor),
                edge_style
            ));
        }
    }

    // 添加轮次分组（使用 subgraph）
    dot.push_str("\n  // Round grouping\n");
    for round in 0..=builder.highest_round() {
        dot.push_str(&format!("  {{rank=same; "));
        let blocks = builder.get_blocks_at_round(round);
        for block in blocks {
            dot.push_str(&format!("\"{}\" ", block_id(&block.reference())));
        }
        dot.push_str("}\n");
    }

    // 图例
    dot.push_str("\n  // Legend\n");
    dot.push_str("  subgraph cluster_legend {\n");
    dot.push_str("    label=\"Legend\";\n");
    dot.push_str("    style=filled;\n");
    dot.push_str("    fillcolor=lightgrey;\n");
    dot.push_str("    \n");
    dot.push_str("    legend_leader [label=\"Leader Round\", style=\"filled,bold\", fillcolor=white];\n");
    dot.push_str("    legend_strong [label=\"Strong Link\", shape=plaintext];\n");
    dot.push_str("    legend_weak [label=\"Weak Link\", shape=plaintext];\n");
    dot.push_str("    legend_strong -> legend_weak [style=solid, label=\"strong\"];\n");
    dot.push_str("    legend_weak -> legend_leader [style=dashed, label=\"weak\"];\n");
    dot.push_str("  }\n");

    dot.push_str("}\n");

    dot
}

fn block_id(block_ref: &BlockRef) -> String {
    format!("R{}_A{}", block_ref.round, block_ref.author.value())
}
