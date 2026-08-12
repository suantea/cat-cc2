// 全流程集成测试：connect → status → 保存规则自动重启 → disconnect
use cat_cc2_lib::core_mgr::{connect, disconnect, get_effective_rules, save_rules, status};

fn test_sub() -> String {
    // ss + 带 obfs plugin 的 ss + vmess 三个假节点，整体 base64
    let ss = "ss://YWVzLTI1Ni1nY206cGFzczEyM0AxLjIuMy40OjgzODgjVGVzdA==";
    // SIP002 plugin 节点：经 mihomo -t 校验 plugin/plugin-opts 配置格式
    let ss_plugin = format!(
        "ss://{}@6.6.6.6:8388/?plugin=obfs-local%3Bobfs%3Dhttp%3Bobfs-host%3Dcdn.example.com#Plugin",
        base64_enc(b"aes-256-gcm:pass123")
    );
    let vmess_json = r#"{"ps":"V2","add":"5.6.7.8","port":443,"id":"u1","aid":0,"net":"ws","host":"h.com","path":"/p","tls":"tls","scy":"auto"}"#;
    let vmess = format!("vmess://{}", base64_enc(vmess_json.as_bytes()));
    base64_enc(format!("{}\n{}\n{}", ss, ss_plugin, vmess).as_bytes())
}

fn base64_enc(bytes: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

#[test]
fn full_flow_connect_disconnect() {
    // 1) connect
    let r = connect(test_sub()).expect("connect 应该成功");
    assert_eq!(r["ok"], true);
    let mixed = r["mixedPort"].as_u64().unwrap_or(0);
    println!("connect ok, mixedPort={}", mixed);

    // 2) 等待内核端口就绪
    std::thread::sleep(std::time::Duration::from_millis(3000));

    // 3) status 应 running=true
    let st = status().expect("status 应可用");
    println!("status: running={} node={} latency={} err={}", st.running, st.node, st.latency, st.last_error);
    assert!(st.running, "内核应处于运行状态");

    // 3.5) 完整生效规则应包含 gfw 代理清单与默认直连兜底
    let eff = get_effective_rules().expect("get_effective_rules 应可用");
    let rules = eff["rules"].as_str().unwrap_or("");
    let gfw_count = eff["gfw_count"].as_u64().unwrap_or(0);
    println!("effective rules: gfw_count={} 含DOMAIN-SUFFIX,auto={} 含MATCH,DIRECT={} 总行数={}",
        gfw_count, rules.contains("DOMAIN-SUFFIX") && rules.contains(",auto"), rules.contains("MATCH,DIRECT"), rules.lines().count());
    assert!(gfw_count > 0, "应包含 gfw 代理清单");
    assert!(rules.contains("MATCH,DIRECT"), "应有默认直连兜底");
    assert!(rules.contains(",auto"), "应有走代理规则");
    assert!(rules.lines().count() > 10, "完整规则应远多于用户自定义部分");

    // 4) 保存规则并自动重启（连接中）
    let rules = "IP-CIDR,127.0.0.0/8,DIRECT\nMATCH,DIRECT".to_string();
    let sr = save_rules(rules, true).expect("save_rules 应成功");
    assert_eq!(sr["ok"], true);
    assert_eq!(sr["restarted"], true, "连接中保存规则应自动重启内核");
    println!("save_rules ok, restarted={}", sr["restarted"]);

    // 5) 重启后仍应运行
    std::thread::sleep(std::time::Duration::from_millis(3000));
    let st2 = status().expect("status 应可用");
    println!("重启后 status: running={} err={}", st2.running, st2.last_error);
    assert!(st2.running, "保存规则重启后内核应继续运行");

    // 6) disconnect
    let d = disconnect().expect("disconnect 应成功");
    assert_eq!(d["ok"], true);

    // 7) status 应 running=false
    let st3 = status().expect("status 应可用");
    assert!(!st3.running, "断开后应停止");
    println!("disconnect ok, running={}", st3.running);
}
