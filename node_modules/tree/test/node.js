module('Node', {
  
  setup: function() {
    this.node1 = new Tree.Node();
    this.node2 = new Tree.Node();
    this.node3 = new Tree.Node();
    this.branch = new Tree.Branch([this.node1, this.node2, this.node3]);
  },
  
  teardown: function() {
    delete this.node1;
    delete this.node2;
    delete this.node3;
    delete this.branch;
  }
  
});

test('set index', function() {
  this.node1.setIndex(1);
  
  equals(this.branch.size(), 3);
  equals(_.first(this.branch.nodes), this.node2);
  equals(_.last(this.branch.nodes), this.node3);

  var error;
  try {
    var node = new Tree.Node({});
    node.setIndex(1);
  } catch (ex) {
    error = ex;
  }
  
  ok(error instanceof TypeError, 'handles a node with no branch');
});