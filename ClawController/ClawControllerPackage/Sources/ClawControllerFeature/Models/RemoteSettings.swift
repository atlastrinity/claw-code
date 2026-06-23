//
//  RemoteSettings.swift
//  ClawControllerFeature
//
//  Model for remote system settings
//

import Foundation

/// Settings for the remote system connection
@Observable
public final class RemoteSettings {
    public var host: String
    public var port: Int
    public var username: String
    public var password: String?
    public var useSsh: Bool
    public var timeout: TimeInterval
    public var autoConnect: Bool
    public var reconnectAttempts: Int

    public init(
        host: String = "localhost",
        port: Int = 22,
        username: String = "user",
        password: String? = nil,
        useSsh: Bool = true,
        timeout: TimeInterval = 30.0,
        autoConnect: Bool = false,
        reconnectAttempts: Int = 3
    ) {
        self.host = host
        self.port = port
        self.username = username
        self.password = password
        self.useSsh = useSsh
        self.timeout = timeout
        self.autoConnect = autoConnect
        self.reconnectAttempts = reconnectAttempts
    }
}
