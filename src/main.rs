use esp_idf_svc::hal::{
    delay::FreeRtos,
    gpio::PinDriver,
    i2s::{
        config::{
            Config, DataBitWidth, PdmRxClkConfig, PdmRxConfig, PdmRxGpioConfig, PdmRxSlotConfig,
            SlotMode,
        },
        I2sDriver, I2sRx,
    },
    prelude::Peripherals,
};
mod wifi;
use base64_stream::ToBase64Reader;
use embedded_svc::http::client::Client;
use esp_idf_svc::{eventloop::EspSystemEventLoop, http::client::EspHttpConnection};
use std::io::{Cursor, Read};
use wifi::wifi;

static SSID: &str = "your-ssid";
static WIFI_PASSWORD: &str = "your-password";
static URL: &str = "http://192.168.86.48:8000";
static HEADERS: [(&str, &str); 1] = [("content-type", "text/plain")];

fn main() {
    // It is necessary to call this function once. Otherwise some patches to the runtime
    // implemented by esp-idf-sys might not link properly. See https://github.com/esp-rs/esp-idf-template/issues/71
    esp_idf_svc::sys::link_patches();

    // Bind the log crate to the ESP Logging facilities
    esp_idf_svc::log::EspLogger::initialize_default();

    let pdm_config = PdmRxConfig::new(
        Config::default(),
        PdmRxClkConfig::from_sample_rate_hz(44100),
        PdmRxSlotConfig::from_bits_per_sample_and_slot_mode(DataBitWidth::Bits16, SlotMode::Mono),
        PdmRxGpioConfig::new(false),
    );
    let peripherals = Peripherals::take().unwrap();
    let din = peripherals.pins.gpio21;
    let clk = peripherals.pins.gpio22;

    // Initialize wifi.
    let _wifi = wifi(
        SSID,
        WIFI_PASSWORD,
        peripherals.modem,
        EspSystemEventLoop::take().unwrap(),
    );

    // An I2S bus that communicates in standard or TDM mode consists of the following lines:
    //
    // MCLK: Master clock line. It is an optional signal depending on the slave side, mainly used for offering a reference clock to the I2S slave device.
    // BCLK: Bit clock line. The bit clock for data line.
    // WS: Word (Slot) select line. It is usually used to identify the vocal tract except PDM mode.
    // DIN/DOUT: Serial data input/output line. Data will loopback internally if DIN and DOUT are set to a same GPIO.
    let mut i2s = I2sDriver::<I2sRx>::new_pdm_rx(peripherals.i2s0, &pdm_config, clk, din).unwrap();
    i2s.rx_enable().unwrap();

    let pin_btn_a = PinDriver::input(peripherals.pins.gpio39).unwrap();
    let mut prev_btn_a_is_low = false;

    let mut request: Option<embedded_svc::http::client::Request<&mut EspHttpConnection>> = None;
    let mut client;

    let mut n_write_total = 0;
    loop {
        if pin_btn_a.is_low() && !prev_btn_a_is_low {
            println!("Button A is pressed");

            prev_btn_a_is_low = true;
            client = Client::wrap(EspHttpConnection::new(&Default::default()).unwrap());
            request = Some(client.post(URL, &HEADERS).unwrap());
        }

        if let Some(ref mut req) = request {
            if pin_btn_a.is_low() {
                let mut buf: [u8; 4800] = [0; 4800];
                let n_bytes = i2s.read(&mut buf, 2).unwrap();
                println!("Read {} bytes", n_bytes);
                let base64 = encode_base64(buf.as_ref());
                println!("base64 length: {}", base64.len());

                let n_write = req.write(&base64.as_bytes()).unwrap();
                FreeRtos::delay_ms(20);
                println!("Write {} bytes", n_write);
                n_write_total += n_write;
            } else if prev_btn_a_is_low && pin_btn_a.is_high() {
                println!("Button A is released");

                req.flush().unwrap();
                println!("-> POST {}", URL);
                let response = request.take().unwrap().submit().unwrap();
                println!("Total: write {} bytes", n_write_total);

                // Process response
                let status = response.status();
                println!("<- {}", status);

                prev_btn_a_is_low = false;
            }
        }
        FreeRtos::delay_ms(1);
    }
}

fn encode_base64(buf: &[u8]) -> String {
    let mut reader = ToBase64Reader::new(Cursor::new(buf));
    let mut base64 = String::new();
    reader.read_to_string(&mut base64).unwrap();
    base64
}
