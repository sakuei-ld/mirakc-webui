use serde::Serialize;
use std::io::Read;
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, Copy)]
pub struct TsHeader {
    pub sync_byte: u8,
    pub transport_error_indicator: bool,
    pub payload_unit_start_indicator: bool,
    pub transport_priority: bool,
    pub pid: u16,
    pub transport_scrambling_control: u8,
    pub adaptation_field_control: u8,
    pub continuity_counter: u8,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct AdaptationField {
    pub length: u8,
    pub discontinuity_counter: u8,
    pub pcr_flag: bool,
    pub pcr: Option<u64>,
    pub splice_countdown: Option<u8>,
    pub random_access_indicator: bool,
}

#[derive(Debug, Default)]
pub struct PidStats {
    pub pid: u16,
    pub total_packets: u64,
    pub drop_count: u64,
    pub error_count: u64,
    pub scrambling_count: u64,
    pub last_continuity_counter: Option<u8>,
    pub last_packet: Option<[u8; 188]>,
    pub duplicate_count: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ErrorDetail {
    pub time: String,
    pub packet_number: u64,
    pub pid: String,
    pub error_type: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DropCheckResult {
    pub total_packets: u64,
    pub total_bytes: u64,
    pub total_drops: u64,
    pub total_errors: u64,
    pub total_scrambling: u64,
    pub syncbyte_lost: u64,
    pub start_pcr: Option<u64>,
    pub end_pcr: Option<u64>,
    pub duration: String,
    pub check_time_ms: u64,
    pub pid_stats: Vec<PidStat>,
    pub details: Vec<ErrorDetail>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PidStat {
    pub pid: String,
    pub packets: u64,
    pub drops: u64,
    pub errors: u64,
    pub scrambling: u64,
}

#[derive(Debug, Clone)]
pub struct CheckOptions {
    pub skip_seconds: u64,
    pub verbose: bool,
    pub limit: usize,
}

impl Default for CheckOptions {
    fn default() -> Self {
        Self {
            skip_seconds: 0,
            verbose: false,
            limit: 16,
        }
    }
}

pub fn parse_ts_header(packet: &[u8; 188]) -> TsHeader {
    TsHeader {
        sync_byte: packet[0],
        transport_error_indicator: (packet[1] & 0x80) != 0,
        payload_unit_start_indicator: (packet[1] & 0x40) != 0,
        transport_priority: (packet[1] & 0x20) != 0,
        pid: (((packet[1] & 0x1f) as u16) << 8) | (packet[2] as u16),
        transport_scrambling_control: (packet[3] & 0x30) >> 4,
        adaptation_field_control: (packet[3] & 0x30) >> 4,
        continuity_counter: packet[3] & 0x0f,
    }
}

pub fn parse_adaptation_field(packet: &[u8], header: &TsHeader) -> Option<AdaptationField> {
    if header.adaptation_field_control != 2 && header.adaptation_field_control != 3 {
        return None;
    }
    
    if packet.len() < 5 || packet[4] == 0 {
        return None;
    }
    
    let mut adapt = AdaptationField::default();
    let mut offset = 5; // adaptation_field_length + 1
    
    adapt.length = packet[4];
    
    while offset < packet.len() && offset < 5 + adapt.length as usize {
        let flag = packet[offset];
        offset += 1;
        
        if flag & 0x80 != 0 && offset + 6 <= packet.len() {
            // PCR field (6 bytes)
            let pcr_base_high = ((packet[offset] as u64) << 25)
                | ((packet[offset + 1] as u64) << 17)
                | ((packet[offset + 2] as u64) << 9)
                | ((packet[offset + 3] as u64) << 1)
                | ((packet[offset + 4] as u64 & 0x80) >> 7);
            let pcr_base_low = ((packet[offset + 4] as u64 & 0x01) << 16)
                | (packet[offset + 5] as u64);
            let pcr_base = (pcr_base_high << 16) | pcr_base_low;
            
            let pcr_ext = if offset + 9 <= packet.len() {
                ((packet[offset + 6] as u64) << 15)
                    | ((packet[offset + 7] as u64) << 7)
                    | ((packet[offset + 8] as u64 & 0x80) >> 1)
            } else {
                0
            };
            
            adapt.pcr = Some((pcr_base << 15) | pcr_ext);
            adapt.pcr_flag = true;
            offset += 8;
        }
        
        if flag & 0x40 != 0 && offset + 1 <= packet.len() {
            adapt.discontinuity_counter = packet[offset];
            offset += 1;
        }
        
        if flag & 0x20 != 0 && offset + 1 <= packet.len() {
            adapt.splice_countdown = Some(packet[offset]);
            offset += 1;
        }
        
        if flag & 0x10 != 0 && offset + 1 <= packet.len() {
            if packet[offset] & 0x80 != 0 {
                adapt.random_access_indicator = true;
            }
            offset += 1;
        }
    }
    
    Some(adapt)
}

fn check_drop(stats: &mut PidStats, header: &TsHeader, adapt: &AdaptationField, packet: &[u8; 188]) {
    if header.pid == 0x1fff {
        return;
    }
    
    if header.adaptation_field_control == 0 {
        return;
    }
    
    if header.adaptation_field_control == 2 {
        if let Some(last_cc) = stats.last_continuity_counter {
            if last_cc != header.continuity_counter {
                stats.drop_count += 1;
            }
        }
        return;
    }
    
    if adapt.discontinuity_counter != 0 {
        stats.last_continuity_counter = Some(header.continuity_counter);
        return;
    }
    
    if let Some(last_cc) = stats.last_continuity_counter {
        let expected = (last_cc + 1) & 0x0f;
        if header.continuity_counter != expected {
            if header.continuity_counter == last_cc {
                if stats.last_packet.as_ref().map_or(false, |last| *last == *packet) {
                    stats.duplicate_count += 1;
                    if stats.duplicate_count > 1 {
                        stats.drop_count += 1;
                    }
                } else {
                    stats.drop_count += 1;
                }
            } else {
                stats.drop_count += 1;
            }
        } else {
            stats.duplicate_count = 0;
        }
    }
    
    stats.last_continuity_counter = Some(header.continuity_counter);
}

pub fn check_ts_file(path: &Path, options: &CheckOptions) -> Result<DropCheckResult, Box<dyn std::error::Error>> {
    let start_time = std::time::Instant::now();
    let file = std::fs::File::open(path)?;
    let mut reader = std::io::BufReader::new(file);
    let mut buf = [0u8; 64 * 1024];
    let mut stats_map: HashMap<u16, PidStats> = HashMap::new();
    let mut details = Vec::new();
    let mut packet_number = 0u64;
    let mut total_bytes = 0u64;
    let mut syncbyte_lost = 0u64;
    let mut start_pcr: Option<u64> = None;
    let mut end_pcr: Option<u64> = None;
    let skip_until_pcr = options.skip_seconds * 90000;
    
    loop {
        let read_len = reader.read(&mut buf)?;
        if read_len == 0 {
            break;
        }
        total_bytes += read_len as u64;
        
        let mut offset = 0;
        while offset + 188 <= read_len {
            if buf[offset] != 0x47 {
                let next_sync = buf[offset..offset + 188].iter().position(|&b| b == 0x47);
                match next_sync {
                    Some(pos) => offset += pos + 1,
                    None => {
                        syncbyte_lost += 1;
                        break;
                    }
                }
                continue;
            }
            
            let packet: [u8; 188] = buf[offset..offset + 188].try_into().unwrap();
            let header = parse_ts_header(&packet);
            let adapt = parse_adaptation_field(&packet, &header);
            
            if let Some(pcr) = adapt.as_ref().and_then(|a| a.pcr) {
                if start_pcr.is_none() {
                    start_pcr = Some(pcr);
                }
                end_pcr = Some(pcr);
            }
            
            if let Some(start) = start_pcr {
                let current_pcr = adapt.as_ref().and_then(|a| a.pcr).unwrap_or(0);
                if current_pcr < start + skip_until_pcr {
                    offset += 188;
                    packet_number += 1;
                    continue;
                }
            }
            
            let pid_stats = stats_map.entry(header.pid).or_default();
            pid_stats.pid = header.pid;
            pid_stats.total_packets += 1;
            
            if header.transport_error_indicator {
                pid_stats.error_count += 1;
                if options.verbose {
                    details.push(ErrorDetail {
                        time: format!("00:00:00.00"),
                        packet_number,
                        pid: format!("0x{:04x}", header.pid),
                        error_type: "error".to_string(),
                    });
                }
            }
            
            check_drop(pid_stats, &header, &adapt.unwrap_or_default(), &packet);
            
            if header.transport_scrambling_control != 0 {
                pid_stats.scrambling_count += 1;
            }
            
            pid_stats.last_packet = Some(packet);
            
            offset += 188;
            packet_number += 1;
        }
    }
    
    let check_time = start_time.elapsed();
    
    let pid_stats: Vec<PidStat> = stats_map.values().map(|s| PidStat {
        pid: format!("0x{:04x}", s.pid),
        packets: s.total_packets,
        drops: s.drop_count,
        errors: s.error_count,
        scrambling: s.scrambling_count,
    }).collect();
    
    let total_drops: u64 = pid_stats.iter().map(|s| s.drops).sum();
    let total_errors: u64 = pid_stats.iter().map(|s| s.errors).sum();
    let total_scrambling: u64 = pid_stats.iter().map(|s| s.scrambling).sum();
    
    let duration = duration_from_pcr(start_pcr, end_pcr);
    
    Ok(DropCheckResult {
        total_packets: packet_number,
        total_bytes,
        total_drops,
        total_errors,
        total_scrambling,
        syncbyte_lost,
        start_pcr,
        end_pcr,
        duration,
        check_time_ms: check_time.as_millis() as u64,
        pid_stats,
        details,
    })
}

pub fn duration_from_pcr(start: Option<u64>, end: Option<u64>) -> String {
    match (start, end) {
        (Some(s), Some(e)) => {
            let diff = if e > s { e - s } else { (0x1ffffffff - s) + e };
            let total_seconds = diff / 90000;
            let hours = total_seconds / 3600;
            let minutes = (total_seconds % 3600) / 60;
            let seconds = total_seconds % 60;
            let centiseconds = (diff % 90000) * 100 / 90000;
            format!("{:02}:{:02}:{:02}.{:02}", hours, minutes, seconds, centiseconds)
        }
        _ => "00:00:00.00".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_parse_ts_header() {
        let mut packet = [0u8; 188];
        packet[0] = 0x47;
        packet[1] = 0x00; // PID=0 (bits 4-0 = 0x00)
        packet[2] = 0x00;
        packet[3] = 0x01; // CC=1
        let header = parse_ts_header(&packet);
        assert_eq!(header.pid, 0);
        assert_eq!(header.continuity_counter, 1);
        assert_eq!(header.sync_byte, 0x47);
    }
    
    #[test]
    fn test_duration_from_pcr() {
        let dur = duration_from_pcr(Some(0), Some(90000));
        assert_eq!(dur, "00:00:01.00");
    }
}
