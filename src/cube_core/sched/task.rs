// src/cube_core/sched/task.rs

#[derive(Copy, Clone, PartialEq, Debug)]
pub enum State { Free, Ready, Running, Zombie, Blocked }

#[derive(Copy, Clone, PartialEq, Debug)]
pub enum Qos {
    Interactive,
    Initiated,
    Utility,
    Background,
    Idle,
}

impl Qos {
    pub fn base_priority(self) -> u8 {
        match self {
            Qos::Interactive => 120,
            Qos::Initiated   => 100,
            Qos::Utility     => 60,
            Qos::Background  => 20,
            Qos::Idle        => 0,
        }
    }

    pub fn quantum_ticks(self) -> u32 {
        match self {
            Qos::Interactive => 1,
            Qos::Initiated   => 2,
            Qos::Utility     => 3,
            Qos::Background  => 5,
            Qos::Idle        => 1,
        }
    }
}

#[repr(C)]
pub struct Task {
    pub rsp: u64,
    pub state: State,
    pub pid: u64,
    pub qos: Qos,
    pub base_priority: u8,
    pub priority: u8,
    pub quantum: u32,
    pub ticks_waited: u32,
    pub stack_base: u64,
    pub stack_pages: usize,

    /// PML4 для этого процесса. 0 = kernel task (identity, KERNEL_CR3).
    /// Владелец AddressSpace: при reap зомби-задачи освобождается через vmm::destroy_pml4(cr3).
    pub cr3: u64,
    pub user_rsp: u64,
    pub user_rip: u64,
    pub name: [u8; 16],
}

impl Task {
    pub const fn free() -> Self {
        Self {
            rsp: 0, state: State::Free, pid: 0, qos: Qos::Idle,
            base_priority: 0, priority: 0, quantum: 0, ticks_waited: 0,
            stack_base: 0, stack_pages: 0,
            cr3: 0, user_rsp: 0, user_rip: 0,
            name: [0; 16],
        }
    }

    pub fn reset_quantum(&mut self) {
        self.quantum = self.qos.quantum_ticks();
    }

    pub fn is_user(&self) -> bool {
        self.cr3 != 0
    }
}

