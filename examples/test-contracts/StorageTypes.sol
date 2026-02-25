// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

interface StorageTypes {
    function setU8(uint8 val) external;
    function getU8() external view returns (uint8);
    function setU16(uint16 val) external;
    function getU16() external view returns (uint16);
    function setU32(uint32 val) external;
    function getU32() external view returns (uint32);
    function setU64(uint64 val) external;
    function getU64() external view returns (uint64);
    function setU128(uint128 val) external;
    function getU128() external view returns (uint128);
    function setU256(uint256 val) external;
    function getU256() external view returns (uint256);
    function setBool(bool val) external;
    function getBool() external view returns (bool);
    function setAddress(address val) external;
    function getAddress() external view returns (address);
    function setBytes32(bytes32 val) external;
    function getBytes32() external view returns (bytes32);
}
