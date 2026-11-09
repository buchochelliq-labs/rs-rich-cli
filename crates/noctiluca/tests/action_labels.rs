//! Configurable names through the real app and shared control path.
use noctiluca::{adapt::{Adapt,Rule},app::{app,Options},bookmarks::Bookmarks,config::{Config,Images,View},engine::{PageAction,Rect},testing::{FakeEngine,PageBuilder}};
use std::time::{Duration,Instant};

#[test]
fn wrapping_is_a_configurable_adaptation_and_details_keeps_f1() {
    let rule=Rule::parse("wrap-action-labels").expect("registered adaptation");
    assert!(Adapt::default().on(rule));
    assert!(!Adapt::none().on(rule));
    let config=Config::parse("[adapt]\nwrap-action-labels = false\n").unwrap();
    assert!(!config.adapt.on(rule));
    for preset in [noctiluca::config::Preset::Vim,noctiluca::config::Preset::Plain] {
        let (keys,warnings)=noctiluca::actions::keymap(preset,&[]);assert!(warnings.is_empty());
        assert_eq!(noctiluca::actions::keys_of(&keys,"page.labels").unwrap().0,"shift+f1");
        assert_eq!(noctiluca::actions::keys_of(&keys,"actions").unwrap().0,"f1");
    }
}

fn fixture() -> (FakeEngine,std::sync::Arc<std::sync::Mutex<noctiluca::testing::FakeState>>,Vec<i64>) {
    let mut p=PageBuilder::new("https://example.test/labels.html","Labels",640.0,630.0);p.view(0.0,630.0);
    let dialog=p.element(p.body(),"DIV",&[("role","dialog"),("aria-modal","true")],&[("position","fixed")],Some(Rect::new(0.0,0.0,304.0,270.0)));p.paint(dialog,100);
    let mut nodes=Vec::new();
    for (i,name) in ["A long action with complete readable final suffix ALPHA","Another complete action ending in BETA","Disabled choice with a complete final suffix GAMMA"].iter().enumerate() {
        let attrs=if i==2 {vec![("disabled","")]} else {vec![]};
        let button=p.element(dialog,"BUTTON",&attrs,&[],Some(Rect::new(8.0,18.0+i as f64*36.0,280.0,30.0)));
        p.text(button,name,Rect::new(8.0,18.0+i as f64*36.0,280.0,18.0)); nodes.push(button);
    }
    let model=noctiluca::model::build(&p.snapshot(),&mut noctiluca::model::Revisions::new());
    let nodes=model.controls.iter().map(|c|c.node).collect();
    let engine=FakeEngine::from_dir(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots")).unwrap();
    let state=engine.state.clone();state.lock().unwrap().pages.insert("labels".into(),p.snapshot());
    (engine,state,nodes)
}

#[test]
fn narrow_source_popup_has_complete_names_and_one_enabled_target_per_action() {
    let (engine,state,nodes)=fixture();
    let mut driver=app(Box::new(engine),Options{config:Config::default(),bookmarks:Bookmarks::default(),url:Some("labels.html".into()),view:Some(View::Projection),images:Some(Images::None),tunnel:None,control:None}).driver(40,24);
    let deadline=Instant::now()+Duration::from_secs(3);
    loop {driver.settle();if driver.find_text("ALPHA").is_some(){break;}assert!(Instant::now()<deadline,"full label missing: {:?}",driver.screen().plain());std::thread::sleep(Duration::from_millis(10));}
    assert!(driver.find_text("BETA").is_some());assert!(driver.find_text("GAMMA").is_some());
    let first=format!("popup-control-{}",nodes[0]);let second=format!("popup-control-{}",nodes[1]);
    for _ in 0..8 {if driver.focused_name().as_deref()==Some(&first){break;}driver.press("tab");}
    assert_eq!(driver.focused_name().as_deref(),Some(first.as_str()));
    driver.press("tab");assert_eq!(driver.focused_name().as_deref(),Some(second.as_str()));
    driver.press("tab");assert_eq!(driver.focused_name().as_deref(),Some(first.as_str()));
    driver.press("shift+f1");driver.settle();assert!(driver.find_text("Action labels").is_some());
    driver.press("esc");driver.settle();assert_eq!(driver.focused_name().as_deref(),Some(first.as_str()));
    assert!(!state.lock().unwrap().actions.iter().any(|a|matches!(a,PageAction::Key(_))),"local details Escape must not reach source");
    driver.press("enter");
    let deadline=Instant::now()+Duration::from_secs(2);
    loop {driver.settle();if state.lock().unwrap().actions.iter().any(|a|matches!(a,PageAction::Click(n) if *n==nodes[0])){break;}assert!(Instant::now()<deadline);std::thread::sleep(Duration::from_millis(10));}
}
