//
//  SystemInfo.swift
//  ClawControllerFeature
//
//  Model representing system information from the remote controller
//

import Foundation
import Observation

/// System information model
@Observable
public class SystemInfo {
    public var status: ConnectionStatus
    public var version: String
    public var uptime: String
    public var cpuUsage: Double
    public var memoryUsage: Double
    public var lastUpdated: Date

    public init(
        status: ConnectionStatus = .disconnected,
        version: String = "Unknown",
        uptime: String = "0s",
        cpuUsage: Double = 0.0,
        memoryUsage: Double = 0.0,
        lastUpdated: Date = Date()
    ) {
        self.status = status
        self.version = version
        self.uptime = uptime
        self.cpuUsage = cpuUsage
        self.memoryUsage = memoryUsage
        self.lastUpdated = lastUpdated
    }
}
