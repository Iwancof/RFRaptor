use hydro_strike::*;

#[test]
fn test_sample_rx() {
    env_logger::builder()
        .filter_level(log::LevelFilter::Trace)
        .is_test(true)
        .init();
    soapysdr::configure_logging();

    let config = device::config::List {
        devices: vec![device::config::Device::File {
            direction: "Rx".to_string(),
            path: "tests/test_sample_rx.txt".to_string(),
        }],
    };

    let (mut rx, _tx) = device::open_device(config).expect("Failed to open device");

    let packets: Vec<hydro_strike::bluetooth::Bluetooth> =
        rx[0].start_rx().expect("Failed to start rx").collect();

    assert_eq!(packets.len(), 4);

    use hydro_strike::bluetooth::MacAddress;
    let test_mac = [
        MacAddress {
            address: [0x1f, 0x6e, 0x9a, 0x98, 0x48, 0xf6],
        },
        MacAddress {
            address: [0x1f, 0x6e, 0x9a, 0x98, 0x48, 0xf6],
        },
        MacAddress {
            address: [0x5e, 0xb0, 0x41, 0x4e, 0xb8, 0x65],
        },
        MacAddress {
            address: [0x5e, 0xb0, 0x41, 0x4e, 0xb8, 0x65],
        },
    ];

    for (p, m) in packets.iter().zip(test_mac.iter()) {
        if let hydro_strike::bluetooth::PacketInner::Advertisement(ref adv) = p.packet.inner {
            assert_eq!(adv.address, *m);
        }
    }
}
