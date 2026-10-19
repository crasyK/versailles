export class LogicalSize {
  constructor(width, height) {
    this.width = width;
    this.height = height;
    this.type = "Logical";
  }
}

export class PhysicalPosition {
  constructor(x, y) {
    this.x = x;
    this.y = y;
    this.type = "Physical";
  }
}

export class PhysicalSize {
  constructor(width, height) {
    this.width = width;
    this.height = height;
    this.type = "Physical";
  }
}
