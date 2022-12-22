use std::thread;
use std::time::Duration;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use atomic_float::AtomicF32;

use zenoh::prelude::sync::*;
use zenoh::subscriber::Subscriber;
use zenoh::buffers::reader::HasReader;
use cdr::{CdrLe, Infinite};

use carla::client::{Vehicle, ActorBase};
use carla::rpc::VehicleWheelLocation;

use crate::autoware_type;

pub struct VehicleBridge<'a> {
    _vehicle_name: String,
    _actor: Vehicle,
    _subscriber_control_cmd: Subscriber<'a, ()>,
    speed: Arc<AtomicF32>,
}

impl<'a> VehicleBridge<'a> {
    pub fn new(z_session: Arc<Session>, name: String, actor: Vehicle) -> VehicleBridge<'a> {
        let publisher_velocity = z_session
            // TODO: Check whether Zenoh can receive the message
            .declare_publisher(name.clone()+"/rt/vehicle/status/velocity_status")
            .res()
            .unwrap();
        let mut vehicle_actor = actor.clone();
        let speed = Arc::new(AtomicF32::new(0.0));
        let update_speed = speed.clone();
        thread::spawn(move || loop {
            //let transform = vehicle_actor.transform();
            let velocity = vehicle_actor.velocity();
            //let angular_velocity = vehicle_actor.angular_velocity();
            //let accel = vehicle_actor.acceleration();
            let velocity_msg = autoware_type::CurrentVelocity {
                header: autoware_type::StdMsgsHeader {  // TODO: Use correct timestamp
                    ts: autoware_type::TimeStamp { sec: 0, nsec: 0},
                    frameid: String::from(""),
                },
                longitudinal_velocity: velocity.norm(),
                lateral_velocity: 0.0,
                // The heading rate is 1 deg to 0.00866, and the direction is reverse
                heading_rate: vehicle_actor.get_wheel_steer_angle(VehicleWheelLocation::FL_Wheel) * -0.00866, 
            };
            let encoded = cdr::serialize::<_, _, CdrLe>(&velocity_msg, Infinite).unwrap();
            publisher_velocity.put(encoded).res().unwrap();
            update_speed.store(velocity_msg.longitudinal_velocity, Ordering::Relaxed);
            //println!("{}", velocity_msg.longitudinal_velocity);
            // TODO: Check the published rate
            //thread::sleep(Duration::from_millis(1000));
            thread::sleep(Duration::from_millis(33)); // 30Hz
        });
        let mut vehicle_actor = actor.clone();
        let current_speed = speed.clone();
        let subscriber_control_cmd = z_session
            .declare_subscriber(name.clone()+"/rt/external/selected/control_cmd")
            .callback_mut(move |sample| {
                match cdr::deserialize_from::<_, autoware_type::AckermannControlCommand, _>(sample.payload.reader(), cdr::size::Infinite) {
                    Ok(cmd) => {
                        let mut control = vehicle_actor.control();
                        // The algorithm is from https://github.com/hatem-darweesh/op_bridge/blob/ros2/op_bridge/op_ros2_agent.py#L219
                        // TODO: Check whether it works while reverse.
                        let speed_diff = cmd.longitudinal.speed - current_speed.load(Ordering::Relaxed);
                        if speed_diff > 0.0 {
                            control.throttle = 0.75;
                            control.brake = 0.0;
                        } else if speed_diff < 0.0 {
                            control.throttle = 0.0;
                            control.brake = if cmd.longitudinal.speed <= 0.0 { 0.75 } else { 0.01 };
                        }
                        println!("target:{} current:{} diff:{}", cmd.longitudinal.speed, cmd.longitudinal.speed - speed_diff, speed_diff);
                        // TODO: 0.3925 means 22.5 deg, but we should get the maximum steering degree first
                        control.steer = -cmd.lateral.steering_tire_angle / 0.3925; // need to reverse the direction
                        vehicle_actor.apply_control(&control);
                        println!("throttle: {}, break: {}\r", control.throttle, control.brake);
                    },
                    Err(_) => {},
                }
            })
            .res()
            .unwrap();
        let _subscriber_gate_mode = z_session
            .declare_subscriber(name.clone()+"/rt/control/gate_mode_cmd")
            .callback_mut(move |_| {

            })
            .res()
            .unwrap();
        let _subscriber_gear_cmd = z_session
            .declare_subscriber(name.clone()+"/rt/external/selected/gear_cmd")
            .callback_mut(move |_| {

            })
            .res()
            .unwrap();
        VehicleBridge { 
            _vehicle_name: name,
            _actor: actor,
            _subscriber_control_cmd: subscriber_control_cmd,
            speed,
        }
    }
}