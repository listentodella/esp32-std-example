use crc32_v2::crc32;
use esp32_nimble::{uuid128, BLEAdvertisementData, BLEDevice, NimbleProperties, NimbleSub};
use esp_idf_hal::delay::FreeRtos;
use esp_idf_sys as _;
use std::sync::{Arc, Condvar, Mutex};
use std::thread;

fn main() {
    esp_idf_sys::link_patches();

    // Take ownership of device
    let ble_device = BLEDevice::take();

    // Obtain handle for peripheral advertiser
    let ble_advertiser = ble_device.get_advertising();

    // Obtain handle for server
    let server = ble_device.get_server();

    // Define server connect behaviour
    server.on_connect(|server, clntdesc| {
        // Print connected client data
        println!("Connected: {:?}", clntdesc);
        // Update connection parameters
        server
            .update_conn_params(clntdesc.conn_handle(), 24, 48, 0, 60)
            .unwrap();
    });

    // Define server disconnect behaviour
    server.on_disconnect(|_desc, _reason| {
        println!("Disconnected, back to advertising");
    });

    // Create a service with custom UUID
    let my_service = server.create_service(
        uuid128!("cddf0001-30f7-4671-8b43-5e40ba53514a"),
        //uuid128!("9b574847-f706-436c-bed7-fc01eb0965c1")
    );

    // 0002 characteristic to upload the experiment xml data
    let exp_svc_characteristic = my_service.lock().create_characteristic(
        uuid128!("cddf0002-30f7-4671-8b43-5e40ba53514a"),
        NimbleProperties::READ | NimbleProperties::WRITE | NimbleProperties::NOTIFY,
    );
    // 0005(just same as ble block in xmls) characteristic to upload sensor data
    let data_characteristic = my_service.lock().create_characteristic(
        uuid128!("cddf0005-30f7-4671-8b43-5e40ba53514a"),
        NimbleProperties::READ | NimbleProperties::WRITE | NimbleProperties::NOTIFY,
    );

    // Configure Advertiser Data
    ble_advertiser
        .lock()
        .set_data(
            BLEAdvertisementData::new()
                .name("ESP32 Server")
                .add_service_uuid(
                    uuid128!("cddf0001-30f7-4671-8b43-5e40ba53514a"), //uuid128!("9b574847-f706-436c-bed7-fc01eb0965c1")
                ),
        )
        .unwrap();

    // Start Advertising
    ble_advertiser.lock().start().unwrap();

    // (Optional) Print dump of local GATT table
    // server.ble_gatts_show_local();

    // Init a value to pass to characteristic
    // test experiment xml
    let experiment = include_str!("./imu_exp.xml").as_bytes();

    let checksum = crc32(0, &experiment);
    // println!("checksum:{:#x} ", checksum);

    let name = "phyphox".as_bytes();
    let checksum_array = checksum.to_be_bytes();
    let exp_len = experiment.len().to_be_bytes();
    let mut header = Vec::with_capacity(15);
    header.extend_from_slice(&name);
    header.extend_from_slice(&exp_len);
    header.extend_from_slice(&checksum_array);
    let header = header.as_slice().to_owned();

    let pair = Arc::new((Mutex::new(false), Condvar::new()));
    let pair2 = Arc::clone(&pair);

    exp_svc_characteristic.lock().on_subscribe(
        move |this, _conn_desc, nimble_sub: esp32_nimble::NimbleSub| {
            println!("get nimble sub = {:?}", nimble_sub);
            println!("instance = {:?}", this);
            if nimble_sub.contains(NimbleSub::NOTIFY) {
                let (lock, cvar) = &*pair2;
                let mut started = lock.lock().unwrap();
                *started = true;
                cvar.notify_one();
            }
        },
    );

    thread::spawn(move || loop {
        println!("waiting for experiment subscribe...");
        let (lock, cvar) = &*pair;
        let mut started = lock.lock().unwrap();
        while !*started {
            started = cvar.wait(started).unwrap();
        }
        *started = false;
        println!("transfer header...");
        exp_svc_characteristic.lock().set_value(&header).notify();
        // this.notify_with(&header, conn_desc.conn_handle()).unwrap();
        FreeRtos::delay_ms(10);
        println!("transfer xml: len = {}...", experiment.len());

        let chunks = experiment.chunks_exact(20);
        let remainer = chunks.remainder();
        for chunk in chunks {
            exp_svc_characteristic.lock().set_value(chunk).notify();
            // println!("chunk len = {}", chunk.len());
            FreeRtos::delay_ms(1);
        }
        if !remainer.is_empty() {
            exp_svc_characteristic.lock().set_value(remainer).notify();
            // println!("chunk len = {}", remainer.len());
        }
        println!("transfer xml done");
    });

    let data_pair = Arc::new((Mutex::new(false), Condvar::new()));
    let data_pair2 = Arc::clone(&data_pair);
    data_characteristic.lock().on_subscribe(
        move |this, _conn_desc, nimble_sub: esp32_nimble::NimbleSub| {
            println!("get nimble sub = {:?}", nimble_sub);
            println!("instance = {:?}", this);
            if nimble_sub.contains(NimbleSub::NOTIFY) {
                println!("notify the client to recv experiment");
                let (lock, cvar) = &*data_pair2;
                let mut started = lock.lock().unwrap();
                *started = true;
                cvar.notify_one();
            } else {
                println!("client Disconnected");
                let (lock, cvar) = &*data_pair2;
                let mut started = lock.lock().unwrap();
                *started = false;
                cvar.notify_one();
            }
        },
    );

    thread::spawn(move || {
        println!("waiting for data subscribe...");

        let (lock, cvar) = &*data_pair;
        let mut started = lock.lock().unwrap();
        while !*started {
            started = cvar.wait(started).unwrap();
        }
        let mut count = 0i16;
        let mut data = [0i16; 6];
        loop {
            // println!("upload fake data");
            data[0] = count.wrapping_mul(1).wrapping_rem(0x1000);
            data[1] = count.wrapping_mul(3).wrapping_rem(0x1000);
            data[2] = count.wrapping_mul(5).wrapping_rem(0x1000);
            data[3] = count.wrapping_mul(2).wrapping_rem(0x1000);
            data[4] = count.wrapping_mul(4).wrapping_rem(0x1000);
            data[5] = count.wrapping_mul(6).wrapping_rem(0x1000);
            count += 1;
            // let bytes = unsafe { core::slice::from_raw_parts(data.as_ptr() as *const u8, data.len() * 2) };
            let bytes = unsafe { core::mem::transmute::<[i16; 6], [u8; 12]>(data) };
            data_characteristic.lock().set_value(&bytes).notify();
            FreeRtos::delay_ms(10);

            // reserve
            // while *started {
            //     started = cvar.wait(started).unwrap();
            // }
            // *started = true;
        }
    });

    /*  // 直接在on_subscribe里传输小段数据是可行的，但是一旦多了，就会crash，不如上面的多线程版本
        exp_svc_characteristic
            .lock()
            .on_subscribe(|this, conn_desc, nimble_sub| {
                println!("exp svc subscribed: {:?}", nimble_sub);
                if nimble_sub.contains(NimbleSub::NOTIFY) {
                    println!("exp:transfer header...");
                    //this.set_value(&header).notify(); // 入参只接受&Self, 而set_value等需要&mut self
                    this.notify_with(remainer, conn_desc.conn_handle()).unwrap();
                    println!("exp:transfer xml...");

                    let chunks = experiment.chunks_exact(20);
                    let remainer = chunks.remainder();
                    for chunk in chunks {
                        this.notify_with(chunk, conn_desc.conn_handle()).unwrap();
                        // println!("chunk len = {}", chunk.len());
                        FreeRtos::delay_ms(1);
                    }
                    if !remainer.is_empty() {
                        this.notify_with(remainer, conn_desc.conn_handle()).unwrap();
                        // println!("chunk len = {}", remainer.len());
                    }
                }
            });
    */
    loop {
        FreeRtos::delay_ms(10000);
    }
}
