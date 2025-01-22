use rfraptor::*;

use bluetooth::{MacAddress, PacketInner};
use stream::{RxStream, Stream, TxStream};

use std::{
    collections::HashMap,
    sync::mpsc::{Receiver, Sender},
    thread,
    time::Duration,
};

use ratatui::{
    crossterm::event::{self, Event, KeyCode},
    layout::{self, Constraint, Flex, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Clear, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};

static WORLD: std::sync::Mutex<World> = std::sync::Mutex::new(World::new());

struct World {
    from_device: Vec<Receiver<bluetooth::Bluetooth>>,
    to_device: Vec<Sender<bluetooth::Bluetooth>>,
}

impl World {
    const fn new() -> Self {
        Self {
            from_device: Vec::new(),
            to_device: Vec::new(),
        }
    }

    fn channel(&mut self) -> (Sender<bluetooth::Bluetooth>, Receiver<bluetooth::Bluetooth>) {
        let (dev_to_world_tx, dev_to_world_rx) = std::sync::mpsc::channel();
        let (world_to_dev_tx, world_to_dev_rx) = std::sync::mpsc::channel();

        self.from_device.push(dev_to_world_rx);
        self.to_device.push(world_to_dev_tx);

        (dev_to_world_tx, world_to_dev_rx)
    }
}

fn spawn() {
    std::thread::spawn(|| loop {
        let world = WORLD.lock().unwrap();

        for (i, from_device) in world.from_device.iter().enumerate() {
            if let Ok(packet) = from_device.try_recv() {
                for (j, to_device) in world.to_device.iter().enumerate() {
                    if i != j {
                        to_device.send(packet.clone()).unwrap();
                    }
                }
            }
        }

        drop(world);
        std::thread::sleep(std::time::Duration::from_millis(10));
    });
}

pub enum VirtualStream {
    WaitRxStart(RxStream<crate::bluetooth::Bluetooth>),
    WaitTxStart(TxStream<crate::bluetooth::Bluetooth>),
    Ready,
    Started,
}

impl VirtualStream {
    pub fn new() -> Self {
        VirtualStream::Ready
    }
}

impl Default for VirtualStream {
    fn default() -> Self {
        VirtualStream::new()
    }
}

impl Stream for VirtualStream {
    fn start_rx(&mut self) -> anyhow::Result<RxStream<crate::bluetooth::Bluetooth>> {
        match self {
            VirtualStream::WaitRxStart(_) => {
                let rx = core::mem::replace(self, VirtualStream::Started);
                if let VirtualStream::WaitRxStart(rx) = rx {
                    Ok(rx)
                } else {
                    unreachable!()
                }
            }
            VirtualStream::WaitTxStart(_) => anyhow::bail!("Already started as Tx"),
            VirtualStream::Ready => {
                let (tx, rx) = WORLD.lock().unwrap().channel();
                *self = VirtualStream::WaitTxStart(TxStream { sink: tx });
                Ok(RxStream { source: rx })
            }
            VirtualStream::Started => anyhow::bail!("Already started"),
        }
    }

    fn start_tx(&mut self) -> anyhow::Result<TxStream<crate::bluetooth::Bluetooth>> {
        match self {
            VirtualStream::WaitRxStart(_) => anyhow::bail!("Already started as Rx"),
            VirtualStream::WaitTxStart(_) => {
                let tx = core::mem::replace(self, VirtualStream::Started);
                if let VirtualStream::WaitTxStart(tx) = tx {
                    Ok(tx)
                } else {
                    unreachable!()
                }
            }
            VirtualStream::Ready => {
                let (tx, rx) = WORLD.lock().unwrap().channel();
                *self = VirtualStream::WaitRxStart(RxStream { source: rx });
                Ok(TxStream { sink: tx })
            }
            VirtualStream::Started => anyhow::bail!("Already started"),
        }
    }
}

enum ExploitBuilderHandleResult {
    Catched,
    Packet(Box<bluetooth::Bluetooth>),
    Fallthrough,
}

trait PopupExploitBuilder: std::fmt::Debug {
    fn layout(
        &mut self,
        src: MacAddress,
        selected_mac: Option<MacAddress>,
        frame: &mut Frame,
        area: layout::Rect,
    );
    fn handle_events(&mut self, key: KeyCode) -> ExploitBuilderHandleResult;
}

#[derive(Debug)]
struct ExploitContainer {
    name: String,
    description: String,
    // packet: bluetooth::Bluetooth,
    exploit: Box<dyn PopupExploitBuilder>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Window {
    Packets,
    Devices,
    Exploits,
}

struct App {
    // virtual device
    rx_monitor: RxStream<crate::bluetooth::Bluetooth>,
    rx_desc: String,
    tx_monitor: TxStream<crate::bluetooth::Bluetooth>,
    tx_desc: String,

    #[allow(unused)] // for drop
    device: Box<dyn Stream>,

    src: MacAddress,

    // databases
    // packets: PacketDB,
    packets: HashMap<Option<MacAddress>, Vec<bluetooth::Bluetooth>>,
    addresses: Vec<Option<MacAddress>>,
    exploits: Vec<ExploitContainer>,

    // indeces
    window_selected: Window,

    devices_focused: bool,

    // device_index: usize,
    device_state: ListState,
    // packet_index: usize,
    packet_state: ListState,
    // exploit_index: usize,
    exploit_state: ListState,

    exploit_selected: bool,
}

impl App {
    fn from_stream(mut device: Box<dyn Stream>) -> Self {
        Self {
            rx_monitor: device.start_rx().unwrap(),
            rx_desc: "No information available".to_string(),
            tx_monitor: device.start_tx().unwrap(),
            tx_desc: "No information available".to_string(),

            device,

            src: MacAddress {
                address: [0x00, 0x01, 0x00, 0x56, 0x34, 0x12],
            },

            packets: HashMap::new(),
            addresses: Vec::new(),
            exploits: Vec::new(),

            window_selected: Window::Devices,

            devices_focused: false,

            device_state: ListState::default().with_selected(Some(0)),
            packet_state: ListState::default().with_selected(Some(0)),
            exploit_state: ListState::default().with_selected(Some(0)),

            exploit_selected: false,
        }
    }

    fn from_dev_conf(mut device: Box<dyn Stream>, rx_desc: String, tx_desc: String) -> Self {
        Self {
            rx_monitor: device.start_rx().unwrap(),
            rx_desc,
            tx_monitor: device.start_tx().unwrap(),
            tx_desc,

            device,

            src: MacAddress {
                address: [0x00, 0x01, 0x00, 0x56, 0x34, 0x12],
            },

            packets: HashMap::new(),
            addresses: Vec::new(),
            exploits: Vec::new(),

            window_selected: Window::Devices,

            devices_focused: false,

            device_state: ListState::default().with_selected(Some(0)),
            packet_state: ListState::default().with_selected(Some(0)),
            exploit_state: ListState::default().with_selected(Some(0)),

            exploit_selected: false,
        }
    }

    fn eat(&mut self) {
        while let Ok(packet) = self.rx_monitor.source.try_recv() {
            let address = if let crate::bluetooth::PacketInner::Advertisement(ref adv) =
                packet.packet.inner
            {
                Some(adv.address.clone())
            } else {
                None
            };

            if self.packets.contains_key(&address) {
                self.packets.get_mut(&address).unwrap().push(packet.clone());
            } else {
                self.packets.insert(address.clone(), vec![packet.clone()]);
                self.addresses.push(address);
            }
        }
    }

    fn get_color(&self, compare: Window) -> Color {
        if self.window_selected == compare {
            if self.exploit_selected {
                Color::Yellow
            } else {
                Color::Green
            }
        } else {
            Color::Reset
        }
    }

    fn layout_rx(&self, frame: &mut Frame, rx: layout::Rect) {
        let content = Line::from(Span::raw(&self.rx_desc));
        // let content = Paragraph::new(content).block(Block::bordered().title("Rx").fg(Color::Reset));
        frame.render_widget(content, rx);
    }

    fn layout_tx(&self, frame: &mut Frame, tx: layout::Rect) {
        let content = Line::from(Span::raw(&self.tx_desc));
        // let content = Paragraph::new(content).block(Block::bordered().title("Tx").fg(Color::Reset));
        frame.render_widget(content, tx);
    }

    fn get_average_rssi(&self, address: &Option<MacAddress>) -> Option<f32> {
        let packets = self.packets.get(address).unwrap();
        let rssi = packets
            .iter()
            .map(|x| {
                x.bytes_packet.as_ref().and_then(|x| {
                    x.raw
                        .as_ref()
                        .and_then(|x| x.raw.as_ref().map(|x| x.rssi_average))
                })
            })
            .try_fold(0., |v: f32, acc: Option<f32>| Some(v + acc?));

        rssi.map(|x| x / packets.len() as f32)
    }

    fn layout_devices(&mut self, frame: &mut Frame, devices: layout::Rect) {
        let items: Vec<ListItem> = self
            .addresses
            .iter()
            .enumerate()
            .map(|(i, k)| {
                let mut span = vec![];

                span.push(Span::raw(format!("{:>3} ", i)));

                span.push(match k {
                    Some(mac) => {
                        Span::raw(format!("{:<17}", mac)).fg(if mac.database().is_some() {
                            Color::Red
                        } else {
                            Color::Reset
                        })
                    }
                    None => Span::raw(format!("{:<17}", "Unknown")).fg(Color::Yellow),
                });

                if let Some(rssi) = self.get_average_rssi(k) {
                    let mut rssi_content = Span::raw(format!("{:>7.2} dB", rssi));

                    if (..-20.).contains(&rssi) {
                        rssi_content = rssi_content.fg(Color::Red);
                    } else if (-20f32..-8.).contains(&rssi) {
                        rssi_content = rssi_content.fg(Color::Yellow);
                    } else {
                        rssi_content = rssi_content.fg(Color::Green);
                    }

                    span.push(rssi_content);
                }

                let num_packets = self.packets.get(k).unwrap().len();
                let num_content = Span::raw(format!("{:>4} packet(s) ", num_packets));

                let num_content = match num_packets {
                    ..10 => num_content.fg(Color::DarkGray),
                    10..30 => num_content.fg(Color::White),
                    30..50 => num_content.fg(Color::Yellow),
                    50..100 => num_content.fg(Color::Magenta),
                    _ => num_content.fg(Color::Red),
                };

                span.push(num_content);

                if let Some(ref byte_packet) =
                    self.packets.get(k).unwrap().last().unwrap().bytes_packet
                {
                    let timestamp = byte_packet
                        .raw
                        .as_ref()
                        .unwrap()
                        .raw
                        .as_ref()
                        .unwrap()
                        .timestamp;

                    let elapsed = chrono::Utc::now().signed_duration_since(timestamp);

                    let elapsed = Span::raw(format!("{:>3}s", elapsed.num_seconds())).fg(
                        if elapsed.num_seconds() < 10 {
                            Color::White
                        } else {
                            Color::Red
                        },
                    );
                    span.push(elapsed);
                }

                if self
                    .packets
                    .get(k)
                    .unwrap()
                    .first()
                    .unwrap()
                    .bytes_packet
                    .is_some()
                    && self.devices_focused
                {
                    let graph_symbols: Vec<Span> = vec![
                        Span::raw(" "),
                        Span::raw("▁").fg(Color::DarkGray),
                        Span::raw("▂").fg(Color::White),
                        Span::raw("▃").fg(Color::White),
                        Span::raw("▄").fg(Color::Yellow),
                        Span::raw("▅").fg(Color::Yellow),
                        Span::raw("▆").fg(Color::Magenta),
                        Span::raw("▇").fg(Color::Magenta),
                        Span::raw("█").fg(Color::Red),
                        Span::raw("█").fg(Color::Red),
                    ];
                    let update_per = 5;
                    let graph_display_num = if self.devices_focused { 20 } else { 10 };

                    let raw_packets: Vec<&burst::Packet> = self
                        .packets
                        .get(k)
                        .unwrap()
                        .iter()
                        .map(|x| {
                            x.bytes_packet
                                .as_ref()
                                .unwrap()
                                .raw
                                .as_ref()
                                .unwrap()
                                .raw
                                .as_ref()
                                .unwrap()
                        })
                        .collect();
                    // separate per 10 seconds

                    let mut data_base = HashMap::new();
                    let first = raw_packets.first().unwrap().timestamp;

                    for p in raw_packets {
                        let idx = p
                            .timestamp
                            .signed_duration_since(first)
                            .num_seconds()
                            .div_euclid(update_per);

                        data_base.entry(idx).or_insert(Vec::new()).push(p);
                    }

                    let now_idx = chrono::Utc::now()
                        .signed_duration_since(first)
                        .num_seconds()
                        .div_euclid(update_per);

                    let mut rssi_ave_graph = vec![-30.; now_idx as usize + 1];
                    let mut packet_count_graph = vec![0; now_idx as usize + 1];

                    for (idx, packets) in data_base {
                        let rssi_ave = packets.iter().map(|x| x.rssi_average).sum::<f32>()
                            / packets.len() as f32;
                        rssi_ave_graph[idx as usize] = rssi_ave;
                        packet_count_graph[idx as usize] = packets.len();
                    }

                    let rssi_ave_graph = rssi_ave_graph
                        .iter()
                        .map(|x| {
                            let mut idx = ((x + 30.) / 4.) as isize;
                            idx = idx.clamp(0, 9);

                            graph_symbols[idx as usize].clone()
                        })
                        .rev()
                        .take(graph_display_num)
                        .rev()
                        .collect::<Vec<Span>>();

                    let packet_count_graph = packet_count_graph
                        .iter()
                        .map(|x| {
                            let mut idx = (*x as f32 / 2.) as usize;
                            idx = idx.clamp(0, 9);

                            graph_symbols[idx].clone()
                        })
                        .rev()
                        .take(graph_display_num)
                        .rev()
                        .collect::<Vec<Span>>();

                    span.push(Span::raw(" "));
                    span.extend(vec![
                        graph_symbols[0].clone();
                        graph_display_num - rssi_ave_graph.len()
                    ]);
                    span.extend(rssi_ave_graph);

                    span.push(Span::raw(" "));
                    span.extend(vec![
                        graph_symbols[0].clone();
                        graph_display_num - packet_count_graph.len()
                    ]);
                    span.extend(packet_count_graph);

                    // show cfo deviation
                    let fsk = self
                        .packets
                        .get(k)
                        .unwrap()
                        .last()
                        .unwrap()
                        .bytes_packet
                        .as_ref()
                        .unwrap()
                        .raw
                        .as_ref()
                        .unwrap();
                    let cfo = fsk.cfo;
                    let deviation = fsk.deviation;

                    let cfo = Span::raw(format!("{:>10.7}", cfo)).fg(Color::Cyan);
                    let deviation = Span::raw(format!("{:>10.7}", deviation)).fg(Color::Cyan);

                    span.push(Span::raw(" "));
                    span.push(cfo);

                    span.push(Span::raw(" "));
                    span.push(deviation);
                }

                ListItem::new(Line::from_iter(span))
            })
            .collect();

        let description = if self.devices_focused {
            Line::from(Span::raw(format!(
                "  {:>3} {:>17} {:>7}   {:>4}       {:>4} {:>20} {:>20} {:>10} {:>10}",
                "IDX", "MAC", "RSSI", "PACKETS", "TIME", "RSSI_GRAPH", "PACK_GRAPH", "CFO", "DEV"
            )))
        } else {
            Line::from(Span::raw(format!(
                "  {:>3} {:>17} {:>7}   {:>4}       {:>4}",
                "IDX", "MAC", "RSSI", "PACKETS", "TIME",
            )))
        };

        let items = List::new(items)
            // .highlight_style(Style::new().reversed())
            .highlight_symbol(">>")
            .repeat_highlight_symbol(true)
            .fg(self.get_color(Window::Devices));

        // render bordered title
        frame.render_widget(
            Block::bordered()
                .title("Devices")
                .style(Style::default().fg(self.get_color(Window::Devices))),
            devices,
        );

        let [description_area, items_area] =
            Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(devices.inner(
                layout::Margin {
                    horizontal: 1,
                    vertical: 1,
                },
            ));

        frame.render_widget(
            Paragraph::new(description).block(Block::default()),
            description_area,
        );
        frame.render_stateful_widget(items, items_area, &mut self.device_state);

        // frame.render_stateful_widget(items, devices, &mut self.device_state);
    }

    fn layout_devices_verbose(&self, frame: &mut Frame, dev_verbose: layout::Rect) {
        let target = self.addresses[self.device_state.selected().unwrap()].clone();

        let mut content = match target {
            Some(ref mac) => {
                let mut line = vec![Line::from(Span::raw(format!("{mac}")))];
                let info = mac.database();
                match info {
                    Some(info) => {
                        line.push(Line::from(Span::raw(format!("Vendor: {0}", info.vendor))));
                        line.push(Line::from(Span::raw(format!(
                            "Block Type: {0}",
                            info.block_type
                        ))));
                    }
                    None => {
                        line.push(Line::from(Span::raw(format!("Vendor: {0}", "Unknown"))));
                    }
                }

                line
            }
            None => vec![Line::from(Span::raw("Unknown"))],
        };

        if let Some(rssi) = self.get_average_rssi(&target) {
            content.push(Line::from(Span::raw(format!(
                "Average RSSI: {:>7.2}",
                rssi
            ))));
        }

        let content = Paragraph::new(content)
            .block(Block::bordered().title("Device Verbose"))
            .wrap(Wrap { trim: true });

        frame.render_widget(content, dev_verbose);
    }

    fn selected_address(&self) -> &Option<MacAddress> {
        let selected = self.device_state.selected().expect("No device selected");
        self.addresses.get(selected).unwrap()
    }

    fn layout_packets(&mut self, frame: &mut Frame, packets: layout::Rect) {
        let items: Vec<ListItem> = self
            .packets
            .get(self.selected_address())
            .unwrap_or(&Vec::new())
            .iter()
            .enumerate()
            .map(|(i, packet)| {
                let content = match &packet.packet.inner {
                    bluetooth::PacketInner::Advertisement(adv) => {
                        let mut data = String::new();
                        data.push_str(&format!(
                            "{:>3} {}: {} packet(s)",
                            i,
                            adv.pdu_header,
                            adv.data.len()
                        ));

                        data
                    }
                    bluetooth::PacketInner::Unimplemented(x) => {
                        format!("{:>3} Unimplemented: 0x{:x}", i, x)
                    }
                }
                .fg(Color::Reset);
                ListItem::new(content)
            })
            .collect();

        let items = List::new(items)
            .block(Block::bordered().title("Packets"))
            .highlight_style(Style::new().reversed())
            .highlight_symbol(">>")
            .repeat_highlight_symbol(true)
            .fg(self.get_color(Window::Packets));

        frame.render_stateful_widget(items, packets, &mut self.packet_state);
    }

    fn layout_packet_verbose(&self, frame: &mut Frame, packet_verbose: layout::Rect) {
        let target = self
            .packets
            .get(self.selected_address())
            .unwrap_or(&Vec::new())
            .get(self.packet_state.selected().unwrap())
            .cloned()
            .unwrap();

        let rf_info = target.bytes_packet.as_ref().and_then(|byte_packet| {
            byte_packet.raw.as_ref().and_then(|fsk_packet| {
                fsk_packet
                    .raw
                    .as_ref()
                    .map(|burst_packet| (burst_packet.rssi_average, burst_packet.timestamp))
            })
        });

        let mut content = match rf_info {
            None => vec![Line::from(Span::raw(format!("RF Freq: {}", target.freq)))],
            Some((rssi, timestamp)) => vec![
                Line::from(Span::raw(format!(
                    "RF Freq: {}, RSSI: {} dB",
                    target.freq, rssi
                ))),
                // show timestamp as simple format
                Line::from(Span::raw(format!(
                    "Timestamp: {}",
                    timestamp.format("%Y-%m-%d %H:%M:%S")
                ))),
            ],
        };

        match target.packet.inner {
            PacketInner::Advertisement(ref adv) => {
                content.push(Line::from(format!(
                    "PDU Header: {}, Length: {}",
                    adv.pdu_header, adv.length
                )));
                for adv_data in &adv.data {
                    if adv_data.data.iter().all(u8::is_ascii_alphanumeric) {
                        content.push(Line::from(adv_data.data.iter().map(|u| *u as char).fold(
                            "".to_string(),
                            |mut s, c| {
                                s.push(c);
                                s
                            },
                        )));
                    } else {
                        content.push(Line::from(format!(
                            "{:40}|{}",
                            &adv_data
                                .data
                                .iter()
                                .map(|x| format!("{:02x}", x))
                                .collect::<Vec<String>>()
                                .join(" "),
                            &adv_data
                                .data
                                .iter()
                                .map(|x| {
                                    if x.is_ascii() && x.is_ascii_alphanumeric() {
                                        format!("{}", *x as char)
                                    } else {
                                        ".".to_string()
                                    }
                                })
                                .collect::<Vec<String>>()
                                .join(""),
                        )));
                    }
                }
            }
            PacketInner::Unimplemented(x) => {
                content.push(Line::from(format!("Unimplemented: 0x{:x}", x)));
                if let Some(ref bytes) = target.bytes_packet {
                    content.push(Line::from(format!("Length: {}", bytes.bytes.len())));
                }
            }
        }

        let content = Paragraph::new(content)
            .block(Block::bordered().title("Packet Verbose"))
            .wrap(Wrap { trim: true });

        frame.render_widget(content, packet_verbose);
    }

    fn layout_exploits(&mut self, frame: &mut Frame, exploits: layout::Rect) {
        let items: Vec<ListItem> = self
            .exploits
            .iter()
            .enumerate()
            .map(|(i, exploit)| {
                let content =
                    Line::from(Span::raw(format!("{i}: {0}", exploit.name))).fg(Color::Reset);
                ListItem::new(content)
            })
            .collect();

        let items = List::new(items)
            .block(Block::bordered().title("Exploits"))
            .highlight_style(Style::new().reversed())
            .highlight_symbol(">>")
            .repeat_highlight_symbol(true)
            .fg(self.get_color(Window::Exploits));

        frame.render_stateful_widget(items, exploits, &mut self.exploit_state);
    }

    fn layout_exploit_verbose(&self, frame: &mut Frame, exploit_verbose: layout::Rect) {
        let target = self
            .exploits
            .get(self.exploit_state.selected().unwrap())
            .unwrap();

        let content = vec![
            ListItem::new(Line::from(Span::raw(format!("Name: {0}", target.name)))),
            ListItem::new(Line::from(Span::raw(format!(
                "Description: {0}",
                target.description
            )))),
        ];

        let content = List::new(content).block(Block::bordered().title("Exploit Verbose"));

        frame.render_widget(content, exploit_verbose);
    }

    fn layout_all(&mut self, frame: &mut Frame) {
        let [rf, main, log] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Ratio(17, 20),
            Constraint::Ratio(2, 20),
        ])
        .areas(frame.area());

        let rx_tx = Layout::horizontal([Constraint::Ratio(1, 2); 2]);
        let [rx, tx] = rx_tx.areas(rf);

        let split = Layout::horizontal([
            // Constraint::Ratio(8, 32),
            // Constraint::Ratio(2, 4),
            // Constraint::Ratio(2, 8),
            Constraint::Ratio(5, 1),
            Constraint::Ratio(5, 1),
            Constraint::Ratio(3, 1),
        ]);
        let [packets, devies, exploits] = split.areas(main);

        let verbose = Layout::vertical([Constraint::Ratio(4, 5), Constraint::Ratio(1, 5)]);

        let [[packets, packet_verbose], [devices, device_verbose], [exploits, exploit_verbose]] = [
            verbose.areas(packets),
            verbose.areas(devies),
            verbose.areas(exploits),
        ];

        self.layout_rx(frame, rx);
        self.layout_tx(frame, tx);

        self.layout_devices(frame, devices);
        self.layout_devices_verbose(frame, device_verbose);

        self.layout_packets(frame, packets);
        self.layout_packet_verbose(frame, packet_verbose);

        self.layout_exploits(frame, exploits);
        self.layout_exploit_verbose(frame, exploit_verbose);

        let widget = tui_logger::TuiLoggerWidget::default().block(Block::bordered().title("Log"));
        frame.render_widget(widget, log);

        fn popup_area(area: Rect, percent_x: u16, percent_y: u16) -> Rect {
            let vertical = Layout::vertical([Constraint::Percentage(percent_y)]).flex(Flex::Center);
            let horizontal =
                Layout::horizontal([Constraint::Percentage(percent_x)]).flex(Flex::Center);
            let [area] = vertical.areas(area);
            let [area] = horizontal.areas(area);
            area
        }

        if self.exploit_selected {
            let area = popup_area(frame.area(), 50, 50);
            frame.render_widget(Clear, area);

            let addr = self.selected_address().clone();
            let src = self.src.clone();

            let exploit = self
                .exploits
                .get_mut(self.exploit_state.selected().unwrap())
                .unwrap();

            exploit.exploit.layout(src, addr, frame, area);
        }
    }

    fn layout(&mut self, frame: &mut Frame) {
        if self.devices_focused {
            self.layout_devices(frame, frame.area());
        } else {
            self.layout_all(frame);
        }
    }

    fn get_selected_state(&mut self) -> &mut ListState {
        match self.window_selected {
            Window::Devices => &mut self.device_state,
            Window::Packets => &mut self.packet_state,
            Window::Exploits => &mut self.exploit_state,
        }
    }

    fn handle_events(&mut self) -> std::io::Result<bool> {
        if event::poll(Duration::from_secs(0))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == event::KeyEventKind::Press {
                    if self.exploit_selected {
                        let e = self
                            .exploits
                            .get_mut(self.exploit_state.selected().unwrap())
                            .unwrap();
                        // if let Some(packet) = e.exploit.handle_events(key.code) {
                        //     self.tx_monitor.sink.send(packet).unwrap();
                        //     return Ok(false);
                        // }
                        let handle = e.exploit.handle_events(key.code);
                        match handle {
                            ExploitBuilderHandleResult::Catched => {
                                return Ok(false);
                            }
                            ExploitBuilderHandleResult::Packet(packet) => {
                                self.tx_monitor.sink.send(*packet).unwrap();
                            }
                            ExploitBuilderHandleResult::Fallthrough => {}
                        }
                    }

                    match key.code {
                        KeyCode::Char('q') => {
                            if self.exploit_selected {
                                self.exploit_selected = false;
                            } else {
                                return Ok(true);
                            }
                        }
                        KeyCode::Char('d') => {
                            self.window_selected = Window::Devices;
                        }
                        KeyCode::Char('p') => {
                            self.window_selected = Window::Packets;
                        }
                        KeyCode::Char('e') => {
                            self.window_selected = Window::Exploits;
                        }
                        KeyCode::Char('f') => {
                            self.devices_focused = !self.devices_focused;
                        }
                        KeyCode::Char('k') => {
                            self.get_selected_state().select_previous();
                        }
                        KeyCode::Char('j') => {
                            self.get_selected_state().select_next();
                        }
                        KeyCode::Char('g') => self.get_selected_state().select_first(),
                        KeyCode::Char('G') => self.get_selected_state().select_last(),
                        KeyCode::Char('h') => match self.window_selected {
                            Window::Devices => {
                                self.window_selected = Window::Packets;
                            }
                            Window::Packets => {
                                self.window_selected = Window::Exploits;
                            }
                            Window::Exploits => {
                                self.window_selected = Window::Devices;
                            }
                        },
                        KeyCode::Char('l') => match self.window_selected {
                            Window::Devices => {
                                self.window_selected = Window::Exploits;
                            }
                            Window::Packets => {
                                self.window_selected = Window::Devices;
                            }
                            Window::Exploits => {
                                self.window_selected = Window::Packets;
                            }
                        },
                        KeyCode::Enter
                            if self.window_selected == Window::Exploits
                                && !self.exploit_selected =>
                        {
                            self.exploit_selected = true;
                        }
                        KeyCode::Esc => {
                            self.exploit_selected = false;
                        }
                        _ => {}
                    }
                }
            }
        }

        Ok(false)
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tui_logger::init_logger(log::LevelFilter::Trace).unwrap();
    tui_logger::set_default_level(log::LevelFilter::Info);
    soapysdr::configure_logging();

    let real_rf = true;
    let mut app = if real_rf {
        let mut devices = device::open_device(device::config::List {
            devices: vec![device::config::Device::HackRF {
                direction: "Rx".to_string(),
                freq_mhz: 2427,
                serial: "0000000000000000f77c60dc259132c3".to_string(), // serial: "0000000000000000436c63dc38276e63".to_string(),
            }],
        })
        .unwrap();
        // Box::new(devices.pop().unwrap())
        let devices = devices.pop().unwrap();
        App::from_dev_conf(
            Box::new(devices),
            "HackRF: Listening on 2427 MHz".to_string(),
            "HackRF: Transmitting on 2427 MHz".to_string(),
        )
    } else {
        // Box::new(VirtualStream::new())
        App::from_stream(Box::new(VirtualStream::new()))
    };

    #[derive(Debug)]
    struct SimplePacketExploit {
        packet: bluetooth::Bluetooth,
        count: u32,
    }

    impl PopupExploitBuilder for SimplePacketExploit {
        fn layout(
            &mut self,
            _src: MacAddress,
            _mac: Option<MacAddress>,
            frame: &mut Frame,
            area: layout::Rect,
        ) {
            let content = Line::from(Span::raw(format!(
                "Send a greeting message: {}",
                self.count
            )));
            let content = List::new(content).block(Block::bordered().title("Exploit"));

            frame.render_widget(content, area);
        }

        fn handle_events(&mut self, key: KeyCode) -> ExploitBuilderHandleResult {
            match key {
                KeyCode::Enter => {
                    self.count += 1;
                    ExploitBuilderHandleResult::Packet(Box::new(self.packet.clone()))
                }
                _ => ExploitBuilderHandleResult::Fallthrough,
            }
        }
    }

    #[derive(Debug)]
    struct OSCommandInjection {
        cmd: String,
        count: u32,
    }

    impl PopupExploitBuilder for OSCommandInjection {
        fn layout(
            &mut self,
            src: MacAddress,
            dest_addr: Option<MacAddress>,
            frame: &mut Frame,
            area: layout::Rect,
        ) {
            let exploit_area = Block::bordered().title("Exploit").title_style(
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            );
            frame.render_widget(exploit_area, area);

            let area = area.inner(layout::Margin {
                horizontal: 1,
                vertical: 1,
            });

            let [info, cmd] = Layout::vertical([
                Constraint::Length(3), // destination
                Constraint::Min(0),    // command
            ])
            .areas(area);

            let [src_info, dest_info] =
                Layout::horizontal([Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)]).areas(info);

            let content = List::new(Line::from(Span::raw(
                dest_addr
                    .map(|x| format!("{x}"))
                    .unwrap_or("Unknown".to_string()),
            )))
            .block(Block::bordered().title("Destination"));
            frame.render_widget(content, dest_info);

            let content = Line::from(Span::raw(format!("{src}")))
                .fg(Color::Yellow)
                .bold();
            let content = List::new(content).block(Block::bordered().title("Source"));
            frame.render_widget(content, src_info);

            let content = Line::from(Span::raw(self.cmd.to_string()))
                .fg(Color::Yellow)
                .bold();
            let content = List::new(content).block(Block::bordered().title("Send Command"));
            frame.render_widget(content, cmd);
        }

        fn handle_events(&mut self, key: KeyCode) -> ExploitBuilderHandleResult {
            match key {
                KeyCode::Char(c) => {
                    self.cmd.push(c);
                    ExploitBuilderHandleResult::Catched
                }
                KeyCode::Backspace => {
                    self.cmd.pop();
                    ExploitBuilderHandleResult::Catched
                }
                KeyCode::Enter => {
                    self.count += 1;
                    let packet = demo_adv_packet(
                        bluetooth::MacAddress {
                            address: [0x00, 0x01, 0x00, 0x56, 0x34, 0x12],
                        },
                        format!("backdoor:{}", self.cmd).into_bytes(),
                    );
                    ExploitBuilderHandleResult::Packet(Box::new(packet))
                }
                _ => ExploitBuilderHandleResult::Fallthrough,
            }
        }
    }

    fn demo_adv_packet(addr: bluetooth::MacAddress, data: Vec<u8>) -> bluetooth::Bluetooth {
        bluetooth::Bluetooth {
            bytes_packet: None,
            packet: bluetooth::BluetoothPacket {
                inner: bluetooth::PacketInner::Advertisement(bluetooth::Advertisement {
                    pdu_header: bluetooth::PDUHeader {
                        pdu_type: bluetooth::PDUType::AdvInd,
                        rfu: false,
                        ch_sel: false,
                        tx_add: false,
                        rx_add: false,
                    },
                    length: data.len() as u8 + 6,
                    address: addr,
                    data: vec![bluetooth::AdvData {
                        len: data.len() as u8,
                        data,
                    }],
                }),
                crc: [0, 0, 0],
            },
            remain: Vec::new(),
            freq: 2427,
        }
    }

    app.exploits.push(ExploitContainer {
        name: "Greetings".to_string(),
        description: "Send a greeting message".to_string(),
        exploit: Box::new(SimplePacketExploit {
            count: 0,
            packet: demo_adv_packet(
                bluetooth::MacAddress {
                    address: [0x00, 0x01, 0x00, 0x56, 0x34, 0x12],
                },
                b"hello:World".to_vec(),
            ),
        }),
    });

    app.exploits.push(ExploitContainer {
        name: "OS Command Injection".to_string(),
        description: "Send command".to_string(),
        exploit: Box::new(OSCommandInjection {
            count: 0,
            cmd: "".to_string(),
        }),
    });

    let mut alice = VirtualStream::new();
    let mut bob = VirtualStream::new();

    if !real_rf {
        let _alice_handle = thread::Builder::new()
            .name("Alice".to_string())
            .spawn(move || {
                let address = bluetooth::MacAddress {
                    address: [0x01, 0x00, 0x00, 0x56, 0x34, 0x12],
                };

                let _rx = alice.start_rx().unwrap();
                let tx = alice.start_tx().unwrap();

                for i in 0.. {
                    let packet =
                        demo_adv_packet(address.clone(), format!("Alice: {}", i).into_bytes());
                    tx.sink.send(packet).unwrap();

                    thread::sleep(Duration::from_secs(1));
                }
                //
            });

        let _bob_handle = thread::Builder::new()
            .name("Bob".to_string())
            .spawn(move || {
                let address = bluetooth::MacAddress {
                    address: [0x02, 0x00, 0x00, 0x56, 0x34, 0x12],
                };

                let rx = bob.start_rx().unwrap();
                let tx = bob.start_tx().unwrap();

                tx.sink
                    .send(demo_adv_packet(address.clone(), b"Bob: Hello".to_vec()))
                    .unwrap();

                // echo server
                for packet in rx.source.iter() {
                    if let bluetooth::PacketInner::Advertisement(adv) = packet.packet.inner {
                        let data = String::from_utf8_lossy(&adv.data[0].data).to_string();

                        // backdoor
                        if data.starts_with("backdoor:") {
                            let cmd = data.trim_start_matches("backdoor:");

                            let mut cmd = cmd.split_whitespace();
                            let mut process = std::process::Command::new(cmd.next().unwrap());

                            for c in cmd {
                                process.arg(c);
                            }

                            let stdout = process
                                .output()
                                .map(|x| x.stdout)
                                .unwrap_or(b"Command Fail".to_vec());
                            let packet = demo_adv_packet(address.clone(), stdout);

                            tx.sink.send(packet).unwrap();
                        } else if data.starts_with("hello:") {
                            let packet = demo_adv_packet(address.clone(), b"Hello: World".to_vec());
                            tx.sink.send(packet).unwrap();
                        }
                    }
                }
            });

        spawn();
    }

    let mut terminal = ratatui::init();

    loop {
        app.eat();

        if app.addresses.is_empty() {
            continue;
        }

        terminal.draw(|frame| {
            app.layout(frame);
        })?;

        if app.handle_events()? {
            break;
        }
    }

    ratatui::restore();

    drop(app);
    thread::sleep(Duration::from_millis(100)); // wait for the thread to finish

    Ok(())
}
